#ifndef WPD_TASK_H
#define WPD_TASK_H

#include "wpd_thread.h"

#include <stdatomic.h>

/* A pool of worker threads shared by everything a decode can hand off. Work is
   submitted as a job: one function run once per index in [0, count), on
   whichever threads are free. The thread that submitted a job joins in while
   waiting for it, so a pool of n threads is n-1 workers plus the caller, and a
   pool nobody can feed still finishes the work rather than deadlocking.

   A pool belongs to one decoder and is not otherwise shared. Every entry point
   here accepts a NULL pool, which runs the job inline on the calling thread,
   so a caller needs no second code path for a build without threads, for
   n_threads == 1, or for a pool that could not be created. */

typedef struct WpdTaskPool WpdTaskPool;

/* Returns 0, or a negative error. Called once per index, from any thread. */
typedef int (*wpd_task_func)(void *arg, int index);

typedef struct WpdTaskJob {
    struct WpdTaskJob *next;
    wpd_task_func      func;
    void              *arg;
    int                count;
#if WPD_HAVE_THREADS
    atomic_int claimed;
    atomic_int finished;
#endif
    atomic_int error;
} WpdTaskJob;

/* 'n_threads' counts the calling thread, so anything below 2 gets no pool and
   NULL is returned; so is a pool that could not be started. */
WpdTaskPool *wpd_task_pool_create(int n_threads);
void         wpd_task_pool_free(WpdTaskPool *pool);
int          wpd_task_pool_threads(const WpdTaskPool *pool);

/* Hands 'job' to the pool and returns immediately. 'job' and everything
   'arg' reaches must stay alive and unchanged until wpd_task_wait() has
   returned. Without a pool the whole job runs here instead. */
void wpd_task_submit(WpdTaskPool *pool, WpdTaskJob *job, wpd_task_func func,
                     void *arg, int count);

/* Runs whatever of 'job' is still unclaimed, waits for the rest, and reports
   the first error any index returned. Must be called exactly once for every
   wpd_task_submit(), including on the error paths of the work that ran
   alongside it. */
int wpd_task_wait(WpdTaskPool *pool, WpdTaskJob *job);

#endif
