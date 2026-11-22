/*
 * Standalone filesystem-only prototype, not a Velella execution backend.
 * Rules constrain this new process and its future children, never its parent.
 * Network, process namespaces, metadata and IPC are outside this prototype.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <linux/landlock.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

#if !defined(SYS_landlock_create_ruleset) || !defined(SYS_close_range)
#error "Linux headers with Landlock and close_range syscall definitions required"
#endif
#ifndef LANDLOCK_ACCESS_FS_TRUNCATE
#error "Linux headers with Landlock ABI 3 filesystem rights required"
#endif

/* Explicit ABI 3 coverage. Requiring ABI >= 3 avoids silently losing truncation
 * protection. Future ABI rights are not claimed by this limited prototype. */
static const uint64_t handled =
    LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_WRITE_FILE |
    LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR |
    LANDLOCK_ACCESS_FS_REMOVE_DIR | LANDLOCK_ACCESS_FS_REMOVE_FILE |
    LANDLOCK_ACCESS_FS_MAKE_CHAR | LANDLOCK_ACCESS_FS_MAKE_DIR |
    LANDLOCK_ACCESS_FS_MAKE_REG | LANDLOCK_ACCESS_FS_MAKE_SOCK |
    LANDLOCK_ACCESS_FS_MAKE_FIFO | LANDLOCK_ACCESS_FS_MAKE_BLOCK |
    LANDLOCK_ACCESS_FS_MAKE_SYM | LANDLOCK_ACCESS_FS_REFER |
    LANDLOCK_ACCESS_FS_TRUNCATE;

struct rule {
    const char *path;
    int writable;
};

static int fail(const char *message) {
    perror(message);
    return 125; /* Never execute the requested command after setup failure. */
}

static int beneath(const char *path, const char *parent) {
    size_t length = strlen(parent);
    return strncmp(path, parent, length) == 0 &&
           (path[length] == '\0' || path[length] == '/');
}

int main(int argc, char **argv) {
    int abi = (int)syscall(SYS_landlock_create_ruleset, NULL, 0,
                          LANDLOCK_CREATE_RULESET_VERSION);
    if (argc == 2 && strcmp(argv[1], "--abi") == 0) {
        int error = abi < 0 ? errno : 0;
        printf("{\"abi\":%d,\"errno\":%d}\n", abi, error);
        return abi >= 3 ? 0 : 2;
    }
    if (abi < 3) {
        fprintf(stderr, "Landlock ABI >= 3 required; command not started\n");
        return 125;
    }

    struct rule rules[64];
    size_t count = 0;
    const char *home = NULL;
    int index = 1;
    for (; index < argc && strcmp(argv[index], "--") != 0; index += 2) {
        if (index + 1 >= argc || count == 64 ||
            (strcmp(argv[index], "--ro") != 0 &&
             strcmp(argv[index], "--rw") != 0)) {
            fprintf(stderr, "Expected --ro/--rw PATH before -- COMMAND\n");
            return 125;
        }
        const char *path = argv[index + 1];
        char canonical[PATH_MAX];
        if (path[0] != '/' || strcmp(path, "/") == 0 ||
            !realpath(path, canonical) || strcmp(path, canonical) != 0) {
            fprintf(stderr, "Rule path must be a canonical absolute non-root directory\n");
            return 125;
        }
        for (size_t prior = 0; prior < count; prior++) {
            if (beneath(path, rules[prior].path) ||
                beneath(rules[prior].path, path)) {
                fprintf(stderr, "Overlapping filesystem rules are not supported\n");
                return 125;
            }
        }
        int writable = strcmp(argv[index], "--rw") == 0;
        rules[count++] = (struct rule){.path = path, .writable = writable};
        if (writable && home == NULL)
            home = path;
    }
    if (home == NULL || index + 1 >= argc || argv[index + 1][0] != '/') {
        fprintf(stderr, "A writable HOME and an absolute command after -- are required\n");
        return 125;
    }

    /* ABI 1-3 create_ruleset uses the first eight bytes only; network access is
     * intentionally not handled. Do not label this as network isolation. */
    struct {
        uint64_t handled_access_fs;
    } attr = {.handled_access_fs = handled};
    int ruleset = (int)syscall(SYS_landlock_create_ruleset, &attr, sizeof(attr), 0);
    if (ruleset < 0)
        return fail("create Landlock ruleset");
    for (size_t i = 0; i < count; i++) {
        int fd = open(rules[i].path, O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
        if (fd < 0) {
            int saved_errno = errno;
            close(ruleset);
            errno = saved_errno;
            return fail("open rule directory");
        }
        uint64_t allowed = LANDLOCK_ACCESS_FS_EXECUTE |
                           LANDLOCK_ACCESS_FS_READ_FILE |
                           LANDLOCK_ACCESS_FS_READ_DIR;
        if (rules[i].writable)
            allowed = handled & ~(LANDLOCK_ACCESS_FS_MAKE_CHAR |
                                  LANDLOCK_ACCESS_FS_MAKE_BLOCK);
        struct landlock_path_beneath_attr path_attr = {
            .allowed_access = allowed, .parent_fd = fd};
        int result = (int)syscall(SYS_landlock_add_rule, ruleset,
                                 LANDLOCK_RULE_PATH_BENEATH, &path_attr, 0);
        int saved_errno = errno;
        close(fd);
        if (result < 0) {
            close(ruleset);
            errno = saved_errno;
            return fail("add Landlock directory rule");
        }
    }
    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) {
        int saved_errno = errno;
        close(ruleset);
        errno = saved_errno;
        return fail("set no_new_privs");
    }
    if (syscall(SYS_landlock_restrict_self, ruleset, 0) != 0) {
        int saved_errno = errno;
        close(ruleset);
        errno = saved_errno;
        return fail("enforce Landlock ruleset");
    }
    close(ruleset);

    /* Landlock does not revoke previously opened file descriptors. Only the
     * caller-provided stdin/stdout/stderr survive; the test runner uses pipes. */
    if (syscall(SYS_close_range, 3U, ~0U, 0) != 0)
        return fail("close inherited descriptors");
    if (clearenv() != 0 || setenv("PATH", "/usr/bin:/bin", 1) != 0 ||
        setenv("LANG", "C.UTF-8", 1) != 0 ||
        setenv("HOME", home, 1) != 0 || setenv("TMPDIR", home, 1) != 0)
        return fail("prepare clean execution environment");
    execv(argv[index + 1], &argv[index + 1]);
    return fail("execute restricted command");
}
