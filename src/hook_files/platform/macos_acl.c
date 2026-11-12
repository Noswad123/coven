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
#include <sys/kauth.h>
#include <sys/vnode.h>
#include <fcntl.h>
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

/* Pinned XNU bsd/man/man2/getattrlist.2 COMMON ATTRIBUTES and
 * bsd/vfs/vfs_attrlist.c getattrlist_common_tab/attr_pack_common:
 * dev_t, fsobj_type_t, uid_t, uint32_t, attrreference_t, uint64_t,
 * each packed at 4 bytes. No RETURNED_ATTRS or PACK_INVAL: unsupported
 * requested metadata must fail EINVAL, not masquerade as an absent ACL.
 */
#pragma pack(push, 4)
struct coven_socket_attrs {
    uint32_t length;
    dev_t device;
    fsobj_type_t type;
    uid_t owner;
    uint32_t mode;
    attrreference_t security;
    uint64_t inode;
};
struct coven_socket_buffer {
    struct coven_socket_attrs fixed;
    unsigned char blob[1024];
};
#pragma pack(pop)
_Static_assert(sizeof(struct coven_socket_attrs) == sizeof(uint32_t) + sizeof(dev_t) +
    sizeof(fsobj_type_t) + sizeof(uid_t) + sizeof(uint32_t) + sizeof(attrreference_t) +
    sizeof(uint64_t), "socket attributes have no implicit padding");

static int coven_socket_matches(const struct stat *s, uint64_t device,
    uint64_t inode, uint32_t uid, uint32_t mode)
{
    return S_ISSOCK(s->st_mode) && s->st_uid == uid &&
        (uint32_t)(s->st_mode & 07777) == mode && s->st_nlink == 1 &&
        (uint64_t)s->st_dev == device && (uint64_t)s->st_ino == inode;
}

/* Metadata only. Neither connected nor listening socket FDs identify the
 * filesystem vnode; openat(O_EVTONLY) on VSOCK is not a supported substitute.
 * getattrlistat is available since 10.10, below the existing rename API floor
 * (10.12); neither statement declares a product deployment floor.
 */
int coven_socket_entry_acl_state(int parentfd, const char *leaf,
    uint64_t device, uint64_t inode, uint32_t uid, uint32_t mode)
{
    struct stat parent, before, after;
    struct attrlist request = {0};
    struct coven_socket_buffer buffer = {0};
    struct coven_socket_attrs *a = &buffer.fixed;
    struct kauth_filesec security = {0};
    size_t start, length, header = KAUTH_FILESEC_SIZE(0), expected;
    int64_t offset;
    uint32_t count;
    int state;

    if (!leaf || !*leaf || !strcmp(leaf, ".") || !strcmp(leaf, "..") || strchr(leaf, '/')) {
        errno = EINVAL;
        return -1;
    }
    state = coven_hook_acl_state(parentfd);
    if (state != 0)
        return state;
    if (fstat(parentfd, &parent) != 0)
        return -1;
    if (!S_ISDIR(parent.st_mode) || parent.st_uid != uid ||
        (parent.st_mode & 07777) != 0700 || (uint64_t)parent.st_dev != device) {
        errno = EACCES;
        return -1;
    }
    if (fstatat(parentfd, leaf, &before, AT_SYMLINK_NOFOLLOW) != 0)
        return -1;
    if (!coven_socket_matches(&before, device, inode, uid, mode)) {
        errno = EACCES;
        return -1;
    }
    request.bitmapcount = ATTR_BIT_MAP_COUNT;
    request.commonattr = ATTR_CMN_DEVID | ATTR_CMN_OBJTYPE | ATTR_CMN_OWNERID |
        ATTR_CMN_ACCESSMASK | ATTR_CMN_EXTENDED_SECURITY | ATTR_CMN_FILEID;
    if (getattrlistat(parentfd, leaf, &request, &buffer, sizeof(buffer),
        FSOPT_NOFOLLOW | FSOPT_REPORT_FULLSIZE) != 0)
        return -1; /* Including ENOENT: never infer ACL absence from an error. */
    if (a->length < sizeof(*a) || a->length > sizeof(buffer))
        goto malformed;
    if ((uint64_t)a->device != device || a->inode != inode || a->type != VSOCK ||
        a->owner != uid || (a->mode & ~S_IFMT) != mode) {
        errno = EACCES;
        return -1;
    }
    /* Signed wide arithmetic before any pointer construction; bounded by total. */
    offset = (int64_t)offsetof(struct coven_socket_attrs, security) + a->security.attr_dataoffset;
    if (offset < (int64_t)sizeof(*a) || offset % 4 != 0 || offset > a->length)
        goto malformed;
    start = (size_t)offset;
    length = a->security.attr_length;
    if (length > a->length - start || start + length != a->length)
        goto malformed;
    state = 0;
    if (length != 0) {
        if (length < header || header > sizeof(security))
            goto malformed;
        memcpy(&security, (unsigned char *)&buffer + start, header);
        if (security.fsec_magic != KAUTH_FILESEC_MAGIC)
            goto malformed;
        count = security.fsec_acl.acl_entrycount;
        if (count != KAUTH_FILESEC_NOACL && count > KAUTH_ACL_MAX_ENTRIES)
            goto malformed;
        /* Bound multiplication before deriving the exact SDK-owned length. */
        if (count != KAUTH_FILESEC_NOACL && count > (length - header) / sizeof(struct kauth_ace))
            goto malformed;
        expected = header + (count == KAUTH_FILESEC_NOACL ? 0 : (size_t)count * sizeof(struct kauth_ace));
        if (length != expected)
            goto malformed;
        state = count != 0 && count != KAUTH_FILESEC_NOACL;
    }
    if (fstatat(parentfd, leaf, &after, AT_SYMLINK_NOFOLLOW) != 0)
        return -1;
    if (!coven_socket_matches(&after, device, inode, uid, mode)) {
        errno = EACCES;
        return -1;
    }
    return state;
malformed:
    errno = EINVAL;
    return -1;
}
