#include <erl_nif.h>
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <unistd.h>

static ERL_NIF_TERM write_bytes(ErlNifEnv *env, int argc, const ERL_NIF_TERM argv[]) {
    ErlNifSInt64 fd;
    ErlNifBinary bytes;
    if (argc != 2 || !enif_get_int64(env, argv[0], &fd) ||
        !enif_inspect_binary(env, argv[1], &bytes)) {
        return enif_make_badarg(env);
    }
    if (fd < 0 || fd > INT_MAX) {
        return enif_make_int(env, -EBADF);
    }
    /* POSIX leaves counts above SSIZE_MAX implementation-defined. */
    if (bytes.size > (size_t)SSIZE_MAX) {
        return enif_make_int(env, -EINVAL);
    }
    ssize_t count = write((int)fd, bytes.data, bytes.size);
    int error = errno;
    return enif_make_int64(env, count < 0 ? -(ErlNifSInt64)error : (ErlNifSInt64)count);
}

/* A blocking descriptor must not occupy an ordinary BEAM scheduler. */
static ErlNifFunc functions[] = {
    {"write", 2, write_bytes, ERL_NIF_DIRTY_JOB_IO_BOUND}
};
ERL_NIF_INIT(aspen_syscall, functions, NULL, NULL, NULL, NULL)
