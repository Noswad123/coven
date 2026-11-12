/* Compile alongside the unchanged helper body with COVEN_HOOK_ACL_TEST. */
#include "macos_acl_mock.h"
#include <assert.h>
#include <errno.h>
#include <stdio.h>
#include <string.h>

enum scenario {
    EMPTY, ABSENT, ALLOW, DENY, SELF, INHERIT, MULTIPLE,
    REMOTE, OTHER_FS, NOOWNERS_EMPTY, NOOWNERS_ABSENT, STAT_ERROR, ATTR_ERROR, SHORT, LONG,
    MISSING_COMMON, MISSING_CAPS, EXTRA_COMMON, EXTRA_VOL, EXTRA_DIR,
    EXTRA_FILE, EXTRA_FORK, NO_VALID, NO_SUPPORT,
    ACL_ERROR, ACL_UNSUPPORTED, ACL_INVALID, ACL_NO_ERRNO, ENTRY_ERROR, ENTRY_NULL, ENTRY_ODD,
    FREE_ERROR, ENTRY_AND_FREE_ERROR, COUNT
};
static enum scenario scenario;
static int stats, attrs, gets, entries, frees;
struct mock_acl { int marker; };
static struct mock_acl object;

int fstatfs(int fd, struct statfs *fs)
{
    assert(fd == 42);
    stats++;
    if (scenario == STAT_ERROR) { errno = EBADF; return -1; }
    memset(fs, 0, sizeof(*fs));
    fs->f_flags = scenario == REMOTE ? 0 : MNT_LOCAL;
    if (scenario == NOOWNERS_EMPTY || scenario == NOOWNERS_ABSENT) {
        fs->f_flags |= scenario == NOOWNERS_EMPTY ? MNT_IGNORE_OWNERSHIP : MNT_UNKNOWNPERMISSIONS;
        errno = EBUSY; /* A successful stat must not supply the rejection errno. */
    }
    strcpy(fs->f_fstypename, scenario == OTHER_FS ? "hfs" : "apfs");
    return 0;
}

int fgetattrlist(int fd, struct attrlist *request, void *buffer, size_t size, unsigned long options)
{
    uint32_t length = 4 + sizeof(attribute_set_t) + sizeof(vol_capabilities_attr_t);
    attribute_set_t returned = {ATTR_CMN_RETURNED_ATTRS, ATTR_VOL_CAPABILITIES, 0, 0, 0};
    vol_capabilities_attr_t caps = {{0}, {0}};
    attrs++;
    assert(fd == 42 && options == 0 && size == length);
    assert(request->bitmapcount == ATTR_BIT_MAP_COUNT && request->reserved == 0);
    assert(request->commonattr == ATTR_CMN_RETURNED_ATTRS);
    assert(request->volattr == (ATTR_VOL_INFO | ATTR_VOL_CAPABILITIES));
    assert(!request->dirattr && !request->fileattr && !request->forkattr);
    if (scenario == ATTR_ERROR) { errno = EACCES; return -1; }
    caps.valid[VOL_CAPABILITIES_INTERFACES] = scenario == NO_VALID ? 0 : VOL_CAP_INT_EXTENDED_SECURITY;
    caps.capabilities[VOL_CAPABILITIES_INTERFACES] = scenario == NO_SUPPORT ? 0 : VOL_CAP_INT_EXTENDED_SECURITY;
    switch (scenario) {
    case SHORT: length--; break;
    case LONG: length++; break;
    case MISSING_COMMON: returned.commonattr = 0; break;
    case MISSING_CAPS: returned.volattr = 0; break;
    case EXTRA_COMMON: returned.commonattr |= 1; break;
    case EXTRA_VOL: returned.volattr |= ATTR_VOL_INFO; break;
    case EXTRA_DIR: returned.dirattr = 1; break;
    case EXTRA_FILE: returned.fileattr = 1; break;
    case EXTRA_FORK: returned.forkattr = 1; break;
    default: break;
    }
    memcpy(buffer, &length, 4);
    memcpy((char *)buffer + 4, &returned, sizeof(returned));
    memcpy((char *)buffer + 4 + sizeof(returned), &caps, sizeof(caps));
    return 0;
}

acl_t acl_get_fd_np(int fd, int type)
{
    assert(fd == 42 && type == ACL_TYPE_EXTENDED);
    gets++;
    if (scenario == NOOWNERS_EMPTY) return &object;
    if (scenario == ABSENT || (scenario >= REMOTE && scenario <= NO_SUPPORT)) {
        errno = ENOENT;
        return NULL;
    }
    if (scenario == ACL_ERROR) { errno = EACCES; return NULL; }
    if (scenario == ACL_UNSUPPORTED) { errno = ENOTSUP; return NULL; }
    if (scenario == ACL_INVALID) { errno = EINVAL; return NULL; }
    if (scenario == ACL_NO_ERRNO) { return NULL; }
    return &object;
}

int acl_get_entry(acl_t acl, int selector, acl_entry_t *entry)
{
    assert(acl == &object && selector == ACL_FIRST_ENTRY);
    entries++;
    if (scenario == EMPTY || scenario == NOOWNERS_EMPTY) { errno = EINVAL; return -1; }
    if (scenario == ENTRY_ERROR || scenario == ENTRY_AND_FREE_ERROR) { errno = EACCES; return -1; }
    if (scenario == ENTRY_NULL) { return 0; }
    if (scenario == ENTRY_ODD) { return 1; }
    *entry = &object; /* Every ACE class is opaque to the helper and rejected. */
    return 0;
}

int acl_free(void *acl)
{
    assert(acl == &object);
    frees++;
    errno = EIO; /* Deliberately clobber the entry-query errno. */
    return scenario == FREE_ERROR || scenario == ENTRY_AND_FREE_ERROR ? -1 : 0;
}

int main(void)
{
    for (scenario = EMPTY; scenario < COUNT; scenario++) {
        int result, error;
        stats = attrs = gets = entries = frees = 0;
        errno = 0;
        result = coven_hook_acl_state(42);
        error = errno;
        assert(stats == 1);
        if (scenario >= REMOTE && scenario <= STAT_ERROR) assert(attrs == 0);
        else assert(attrs == 1);
        if (scenario == EMPTY || scenario == ABSENT) assert(result == 0);
        else if (scenario >= ALLOW && scenario <= MULTIPLE) assert(result == 1);
        else assert(result == -1);
        if (scenario >= REMOTE && scenario <= NO_SUPPORT) {
            /* Even if ACL lookup would return ENOENT, unsupported caps stop it. */
            assert(gets == 0 && entries == 0 && frees == 0);
        } else if (scenario == ABSENT || (scenario >= ACL_ERROR && scenario <= ACL_NO_ERRNO)) {
            assert(gets == 1 && entries == 0 && frees == 0);
        } else assert(gets == 1 && entries == 1 && frees == 1);
        if (scenario == ENTRY_ERROR || scenario == ENTRY_AND_FREE_ERROR || scenario == ACL_ERROR || scenario == ATTR_ERROR) assert(error == EACCES);
        if (scenario == ACL_NO_ERRNO || scenario == ENTRY_NULL || scenario == ENTRY_ODD || scenario == FREE_ERROR) assert(error == EIO);
        if (scenario == STAT_ERROR) assert(error == EBADF);
        if (scenario == NOOWNERS_EMPTY || scenario == NOOWNERS_ABSENT) {
            /* Local APFS with otherwise-valid caps and an empty/absent ACL
             * must stop before either capability or ACL lookup.
             */
            assert(error == ENOTSUP && attrs == 0 && gets == 0 && entries == 0 && frees == 0);
        }
        if (scenario == ACL_UNSUPPORTED) assert(error == ENOTSUP);
        if (scenario == ACL_INVALID) assert(error == EINVAL);
        if ((scenario == REMOTE || scenario == OTHER_FS) || (scenario >= SHORT && scenario <= NO_SUPPORT)) assert(error == ENOTSUP);
    }
    printf("macos ACL mock logic: %d scenarios passed (not SDK/native qualification)\n", COUNT);
    return 0;
}
