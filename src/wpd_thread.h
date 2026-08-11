#ifndef WPD_THREAD_H
#define WPD_THREAD_H

#include "wpd_compat.h"

#ifndef WPD_HAVE_THREADS
#define WPD_HAVE_THREADS 0
#endif

#if WPD_HAVE_THREADS

#ifdef _WIN32
#include <process.h>
#include <windows.h>

typedef HANDLE             wpd_thread;
typedef SRWLOCK            wpd_mutex;
typedef CONDITION_VARIABLE wpd_cond;

#define WPD_THREAD_ENTRY(name) static unsigned __stdcall name(void *arg)
typedef unsigned(__stdcall *wpd_thread_func)(void *);

static wpd_always_inline int wpd_mutex_init(wpd_mutex *m) {
    InitializeSRWLock(m);
    return 0;
}

static wpd_always_inline void wpd_mutex_destroy(wpd_mutex *m) { (void)m; }

static wpd_always_inline void wpd_mutex_lock(wpd_mutex *m) {
    AcquireSRWLockExclusive(m);
}

static wpd_always_inline void wpd_mutex_unlock(wpd_mutex *m) {
    ReleaseSRWLockExclusive(m);
}

static wpd_always_inline int wpd_cond_init(wpd_cond *c) {
    InitializeConditionVariable(c);
    return 0;
}

static wpd_always_inline void wpd_cond_destroy(wpd_cond *c) { (void)c; }

static wpd_always_inline void wpd_cond_wait(wpd_cond *c, wpd_mutex *m) {
    SleepConditionVariableSRW(c, m, INFINITE, 0);
}

static wpd_always_inline void wpd_cond_broadcast(wpd_cond *c) {
    WakeAllConditionVariable(c);
}

static wpd_always_inline int wpd_thread_create(wpd_thread     *t,
                                               wpd_thread_func fn, void *arg) {
    *t = (HANDLE)_beginthreadex(NULL, 0, fn, arg, 0, NULL);
    return *t ? 0 : -1;
}

static wpd_always_inline void wpd_thread_join(wpd_thread *t) {
    WaitForSingleObject(*t, INFINITE);
    CloseHandle(*t);
}

#else /* POSIX */
#include <pthread.h>

typedef pthread_t       wpd_thread;
typedef pthread_mutex_t wpd_mutex;
typedef pthread_cond_t  wpd_cond;

#define WPD_THREAD_ENTRY(name) static void *name(void *arg)
typedef void *(*wpd_thread_func)(void *);

static wpd_always_inline int wpd_mutex_init(wpd_mutex *m) {
    return pthread_mutex_init(m, NULL) ? -1 : 0;
}

static wpd_always_inline void wpd_mutex_destroy(wpd_mutex *m) {
    pthread_mutex_destroy(m);
}

static wpd_always_inline void wpd_mutex_lock(wpd_mutex *m) {
    pthread_mutex_lock(m);
}

static wpd_always_inline void wpd_mutex_unlock(wpd_mutex *m) {
    pthread_mutex_unlock(m);
}

static wpd_always_inline int wpd_cond_init(wpd_cond *c) {
    return pthread_cond_init(c, NULL) ? -1 : 0;
}

static wpd_always_inline void wpd_cond_destroy(wpd_cond *c) {
    pthread_cond_destroy(c);
}

static wpd_always_inline void wpd_cond_wait(wpd_cond *c, wpd_mutex *m) {
    pthread_cond_wait(c, m);
}

static wpd_always_inline void wpd_cond_broadcast(wpd_cond *c) {
    pthread_cond_broadcast(c);
}

static wpd_always_inline int wpd_thread_create(wpd_thread     *t,
                                               wpd_thread_func fn, void *arg) {
    return pthread_create(t, NULL, fn, arg) ? -1 : 0;
}

static wpd_always_inline void wpd_thread_join(wpd_thread *t) {
    pthread_join(*t, NULL);
}

#endif /* _WIN32 */

#endif /* WPD_HAVE_THREADS */

#endif
