#include "wpd_task.h"

#include "wpd_util.h"

#include <stdlib.h>

static void job_run_index(WpdTaskJob *job, int index) {
    const int ret = job->func(job->arg, index);

    if (ret < 0) {
        int none = 0;

        atomic_compare_exchange_strong(&job->error, &none, ret);
    }
}

#if WPD_HAVE_THREADS

struct WpdTaskPool {
    wpd_mutex   lock;
    wpd_cond    work;
    wpd_cond    done;
    WpdTaskJob *head, *tail;
    /* Set while a wake-up is posted and not yet consumed, so a burst of
       submissions costs one signal rather than one per job. */
    atomic_int  signaled;
    wpd_thread *thread;
    int         nb_threads;
    int         nb_started;
    int         die;
};

/* Takes the next unclaimed index of 'only', or of any queued job when 'only'
   is NULL. Called with the lock held. */
static WpdTaskJob *pool_claim(WpdTaskPool *pool, WpdTaskJob *only, int *index) {
    for (WpdTaskJob *job = only ? only : pool->head; job; job = job->next) {
        const int claimed = atomic_load_explicit(&job->claimed,
                                                 memory_order_relaxed);

        if (claimed < job->count) {
            atomic_store_explicit(
                &job->claimed, claimed + 1, memory_order_relaxed);
            *index = claimed;
            return job;
        }
        if (only)
            break;
    }
    return NULL;
}

/* Retires 'job' once its last index is in, and wakes whoever waits on it.
   Called with the lock held. */
static void pool_finish(WpdTaskPool *pool, WpdTaskJob *job) {
    if (atomic_fetch_add(&job->finished, 1) + 1 < job->count)
        return;

    WpdTaskJob **link = &pool->head;

    while (*link && *link != job) link = &(*link)->next;
    if (*link)
        *link = job->next;
    pool->tail = NULL;
    for (WpdTaskJob *j = pool->head; j; j = j->next) pool->tail = j;
    wpd_cond_broadcast(&pool->done);
}

WPD_THREAD_ENTRY(pool_worker) {
    WpdTaskPool *pool = arg;

    wpd_mutex_lock(&pool->lock);
    for (;;) {
        WpdTaskJob *job;
        int         index;

        if (pool->die)
            break;
        job = pool_claim(pool, NULL, &index);
        if (!job) {
            atomic_store(&pool->signaled, 0);
            wpd_cond_wait(&pool->work, &pool->lock);
            continue;
        }
        /* Hand the baton on before leaving, so one more thread wakes per unit
           of work found rather than every thread per submission. */
        atomic_store(&pool->signaled, 1);
        wpd_cond_signal(&pool->work);
        wpd_mutex_unlock(&pool->lock);
        job_run_index(job, index);
        wpd_mutex_lock(&pool->lock);
        pool_finish(pool, job);
    }
    wpd_mutex_unlock(&pool->lock);
    return 0;
}

wpd_cold WpdTaskPool *wpd_task_pool_create(int nb_threads) {
    WpdTaskPool *pool;

    if (nb_threads < 2)
        return NULL;
    pool = calloc(1, sizeof(*pool));
    if (!pool)
        return NULL;
    pool->nb_threads = nb_threads - 1;
    pool->thread     = calloc((size_t)pool->nb_threads, sizeof(*pool->thread));
    if (!pool->thread) {
        free(pool);
        return NULL;
    }
    if (wpd_mutex_init(&pool->lock) < 0)
        goto fail_alloc;
    if (wpd_cond_init(&pool->work) < 0)
        goto fail_lock;
    if (wpd_cond_init(&pool->done) < 0)
        goto fail_work;
    return pool;

fail_work:
    wpd_cond_destroy(&pool->work);
fail_lock:
    wpd_mutex_destroy(&pool->lock);
fail_alloc:
    free(pool->thread);
    free(pool);
    return NULL;
}

/* Threads start when a job first has room for them and live until the pool
   does, so a decoder that only ever overlaps two pieces of work pays for one
   worker however many the caller asked to be allowed. Only the thread that
   submits jobs runs this, so nb_started needs no lock. */
static void pool_grow(WpdTaskPool *pool, int count) {
    const int want = WPD_MIN(pool->nb_threads, count);

    while (pool->nb_started < want) {
        if (wpd_thread_create(
                &pool->thread[pool->nb_started], pool_worker, pool) < 0)
            break;
        pool->nb_started++;
    }
}

wpd_cold void wpd_task_pool_free(WpdTaskPool *pool) {
    if (!pool)
        return;
    wpd_mutex_lock(&pool->lock);
    pool->die = 1;
    wpd_cond_broadcast(&pool->work);
    wpd_mutex_unlock(&pool->lock);
    for (int i = 0; i < pool->nb_started; i++)
        wpd_thread_join(&pool->thread[i]);
    wpd_cond_destroy(&pool->done);
    wpd_cond_destroy(&pool->work);
    wpd_mutex_destroy(&pool->lock);
    free(pool->thread);
    free(pool);
}

int wpd_task_pool_threads(const WpdTaskPool *pool) {
    return pool ? pool->nb_threads + 1 : 1;
}

void wpd_task_submit(WpdTaskPool *pool, WpdTaskJob *job, wpd_task_func func,
                     void *arg, int count) {
    job->func  = func;
    job->arg   = arg;
    job->count = count;
    job->next  = NULL;
    atomic_store(&job->error, 0);

    if (pool && count > 0)
        pool_grow(pool, count);
    /* No worker started, so a queued job would wait for a thread that is never
       coming; the caller does it here instead. */
    if (!pool || !pool->nb_started || count <= 0) {
        atomic_store(&job->claimed, count);
        atomic_store(&job->finished, count);
        for (int i = 0; i < count; i++) job_run_index(job, i);
        return;
    }

    atomic_store(&job->claimed, 0);
    atomic_store(&job->finished, 0);
    wpd_mutex_lock(&pool->lock);
    if (pool->tail)
        pool->tail->next = job;
    else
        pool->head = job;
    pool->tail = job;
    if (!atomic_exchange(&pool->signaled, 1))
        wpd_cond_signal(&pool->work);
    wpd_mutex_unlock(&pool->lock);
}

int wpd_task_wait(WpdTaskPool *pool, WpdTaskJob *job) {
    if (!pool || job->count <= 0)
        return atomic_load(&job->error);

    wpd_mutex_lock(&pool->lock);
    /* Only this job's indices: taking someone else's would put an unrelated
       job's runtime, and its failures, on this caller. */
    for (;;) {
        int index;

        if (atomic_load_explicit(&job->finished, memory_order_relaxed) >=
            job->count)
            break;
        if (!pool_claim(pool, job, &index)) {
            wpd_cond_wait(&pool->done, &pool->lock);
            continue;
        }
        wpd_mutex_unlock(&pool->lock);
        job_run_index(job, index);
        wpd_mutex_lock(&pool->lock);
        pool_finish(pool, job);
    }
    wpd_mutex_unlock(&pool->lock);
    return atomic_load(&job->error);
}

#else /* !WPD_HAVE_THREADS */

WpdTaskPool *wpd_task_pool_create(int nb_threads) {
    (void)nb_threads;
    return NULL;
}

void wpd_task_pool_free(WpdTaskPool *pool) { (void)pool; }

int wpd_task_pool_threads(const WpdTaskPool *pool) {
    (void)pool;
    return 1;
}

void wpd_task_submit(WpdTaskPool *pool, WpdTaskJob *job, wpd_task_func func,
                     void *arg, int count) {
    (void)pool;
    job->func  = func;
    job->arg   = arg;
    job->count = count;
    job->next  = NULL;
    atomic_store(&job->error, 0);
    for (int i = 0; i < count; i++) job_run_index(job, i);
}

int wpd_task_wait(WpdTaskPool *pool, WpdTaskJob *job) {
    (void)pool;
    return atomic_load(&job->error);
}

#endif /* WPD_HAVE_THREADS */
