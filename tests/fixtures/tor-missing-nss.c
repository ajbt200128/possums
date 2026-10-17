#include <pwd.h>
#include <errno.h>
static int missing_uid(uid_t uid, struct passwd *pw, char *buf, size_t size, struct passwd **out) {
    *out = 0;
    return ENOENT;
}
static int missing_name(const char *name, struct passwd *pw, char *buf, size_t size, struct passwd **out) {
    *out = 0;
    return ENOENT;
}
__attribute__((used)) static struct { const void *replacement; const void *original; } interposers[]
__attribute__((section("__DATA,__interpose"))) = {
    { (const void *)missing_uid, (const void *)getpwuid_r },
    { (const void *)missing_name, (const void *)getpwnam_r }
};
