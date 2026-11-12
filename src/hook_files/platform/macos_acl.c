/* Public Darwin behavior pinned to Apple Libc
 * a61a5933206342413af76579500f313a045c1c33 (posix1e/acl_{file,entry}.c)
 * and XNU 2c2f96dc2b9a4408a43d3150ae9c105355ca3daa (bsd/sys/{attr,mount}.h).
 * Native builds MUST compile against the Apple SDK, never the mock header.
 */
#ifdef COVEN_HOOK_ACL_TEST
#include "macos_acl_mock.h"
#elif defined(__APPLE__) && defined(__MACH__)
#include <sys/types.h>
#include <sys/acl.h>
#include <sys/attr.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <unistd.h>
#else
#error macos_acl.c requires an Apple SDK (or the explicit logic-test fixture)
#endif
#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* getattrlist's fixed attributes are packed at four-byte boundaries, in
 * bitmap order: length, returned attributes, volume capabilities. Verified in
 * pinned XNU bsd/vfs/vfs_attrlist.c: attrlist_pack_fixed/getvolattrlist.
 * ATTR_VOL_INFO
 * is a request discriminator, not a payload field. SDK types own all fields.
 */
#pragma pack(push, 4)
struct coven_volume_caps {
    uint32_t length;
    attribute_set_t returned;
    vol_capabilities_attr_t caps;
};
#pragma pack(pop)
_Static_assert(offsetof(struct coven_volume_caps, returned) == sizeof(uint32_t), "returned attrs layout");
_Static_assert(offsetof(struct coven_volume_caps, caps) == sizeof(uint32_t) + sizeof(attribute_set_t), "caps layout");
_Static_assert(sizeof(struct coven_volume_caps) == sizeof(uint32_t) + sizeof(attribute_set_t) + sizeof(vol_capabilities_attr_t), "caps trailing padding");

/* 0 = no ACL, 1 = any ACE, -1 = error with errno. No pathname operations,
 * ACL mutation, xattr/flags handling, or full-metadata restoration promise.
 */
int coven_hook_acl_state(int fd)
{
    struct statfs fs;
    struct attrlist request = {0};
    struct coven_volume_caps volume = {0};
    acl_t acl;
    acl_entry_t entry = NULL;
    int result, saved, state;

    if (fstatfs(fd, &fs) != 0)
        return -1;
    /* noowners defeats Root's uid/mode privacy checks even with no ACL.
     * The SDK's MNT_UNKNOWNPERMISSIONS is an alias for this flag.
     */
    if (fs.f_flags & MNT_IGNORE_OWNERSHIP) {
        errno = ENOTSUP;
        return -1;
    }
    if (!(fs.f_flags & MNT_LOCAL) ||
        memcmp(fs.f_fstypename, "apfs\0", sizeof("apfs")) != 0) {
        errno = ENOTSUP;
        return -1;
    }
    request.bitmapcount = ATTR_BIT_MAP_COUNT;
    request.commonattr = ATTR_CMN_RETURNED_ATTRS;
    request.volattr = ATTR_VOL_INFO | ATTR_VOL_CAPABILITIES;
    if (fgetattrlist(fd, &request, &volume, sizeof(volume), 0) != 0)
        return -1;
    if (volume.length != sizeof(volume) ||
        volume.returned.commonattr != ATTR_CMN_RETURNED_ATTRS ||
        volume.returned.volattr != ATTR_VOL_CAPABILITIES ||
        volume.returned.dirattr != 0 || volume.returned.fileattr != 0 ||
        volume.returned.forkattr != 0 ||
        !(volume.caps.valid[VOL_CAPABILITIES_INTERFACES] & VOL_CAP_INT_EXTENDED_SECURITY) ||
        !(volume.caps.capabilities[VOL_CAPABILITIES_INTERFACES] & VOL_CAP_INT_EXTENDED_SECURITY)) {
        errno = ENOTSUP;
        return -1;
    }

    errno = 0;
    acl = acl_get_fd_np(fd, ACL_TYPE_EXTENDED);
    saved = errno;
    if (acl == NULL) {
        /* Only the established local APFS support contract permits ENOENT. */
        if (saved == ENOENT)
            return 0;
        errno = saved ? saved : EIO;
        return -1;
    }
    errno = 0;
    result = acl_get_entry(acl, ACL_FIRST_ENTRY, &entry);
    saved = errno;
    if (result == 0 && entry != NULL)
        state = 1; /* Includes deny-only, self, and inheritance-only ACEs. */
    else if (result == -1 && saved == EINVAL)
        state = 0; /* Valid get object, fixed first-entry selector, zero entries. */
    else {
        state = -1;
        if (result != -1 || saved == 0)
            saved = EIO;
    }
    result = acl_free(acl); /* Exactly once, even on entry-query failure. */
    if (state == -1) {
        errno = saved;
        return -1;
    }
    if (result != 0)
        return -1;
    return state;
}
