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

enum socket_scenario {
    SOCKET_NONE, SOCKET_ZERO, SOCKET_ACE, SOCKET_SENTINEL,
    SOCKET_MAGIC, SOCKET_SHORT_HEADER, SOCKET_COUNT_OVERFLOW, SOCKET_LENGTH,
    SOCKET_TOTAL_SHORT, SOCKET_TOTAL_LONG, SOCKET_OFFSET_NEGATIVE,
    SOCKET_OFFSET_OVERFLOW, SOCKET_OFFSET_UNALIGNED, SOCKET_OFFSET_FIXED,
    SOCKET_SYMLINK, SOCKET_TYPE, SOCKET_UID, SOCKET_MODE, SOCKET_DEV,
    SOCKET_INO, SOCKET_NLINK, SOCKET_REPLACED, SOCKET_QUERY_UNSUPPORTED,
    SOCKET_QUERY_INVALID, SOCKET_QUERY_MISSING, SOCKET_QUERY_DENIED,
    SOCKET_ATTR_TYPE, SOCKET_ATTR_UID, SOCKET_ATTR_MODE, SOCKET_ATTR_DEV,
    SOCKET_ATTR_INO, SOCKET_PARENT_TYPE, SOCKET_PARENT_UID, SOCKET_PARENT_MODE,
    SOCKET_PARENT_DEV, SOCKET_PARENT_STAT, SOCKET_BEFORE_ERROR, SOCKET_AFTER_ERROR,
    SOCKET_AFTER_UID, SOCKET_AFTER_MODE, SOCKET_AFTER_NLINK,
    SOCKET_BLOB_TRUNCATED, SOCKET_REF_LENGTH_OVERFLOW, SOCKET_COUNT_MISMATCH,
    SOCKET_INITIAL_777, SOCKET_INITIAL_700, SOCKET_COUNT
};
static enum socket_scenario socket_scenario;
static int parent_stats, source_stats, socket_attrs;
static uint32_t socket_mode;

int fstat(int fd, struct stat *s)
{
    assert(fd == 42 && stats == 1 && attrs == 1 && gets == 1);
    parent_stats++;
    if (socket_scenario == SOCKET_PARENT_STAT) { errno = EBADF; return -1; }
    memset(s, 0, sizeof(*s));
    s->st_mode = S_IFDIR | 0700;
    s->st_uid = 501;
    s->st_dev = 12;
    switch (socket_scenario) {
    case SOCKET_PARENT_TYPE: s->st_mode = S_IFREG | 0700; break;
    case SOCKET_PARENT_UID: s->st_uid++; break;
    case SOCKET_PARENT_MODE: s->st_mode |= 0001; break;
    case SOCKET_PARENT_DEV: s->st_dev++; break;
    default: break;
    }
    return 0;
}

int fstatat(int fd, const char *leaf, struct stat *s, int flags)
{
    assert(fd == 42 && !strcmp(leaf, "receiver.sock") && flags == AT_SYMLINK_NOFOLLOW);
    assert(parent_stats == 1);
    source_stats++;
    if ((socket_scenario == SOCKET_BEFORE_ERROR && source_stats == 1) ||
        (socket_scenario == SOCKET_AFTER_ERROR && source_stats == 2)) {
        errno = ENOENT; return -1;
    }
    memset(s, 0, sizeof(*s));
    s->st_mode = S_IFSOCK | socket_mode;
    s->st_uid = 501;
    s->st_dev = 12;
    s->st_ino = 34;
    s->st_nlink = 1;
    switch (socket_scenario) {
    case SOCKET_SYMLINK: s->st_mode = S_IFLNK | socket_mode; break;
    case SOCKET_TYPE: s->st_mode = S_IFREG | socket_mode; break;
    case SOCKET_UID: s->st_uid++; break;
    case SOCKET_MODE: s->st_mode |= 0001; break;
    case SOCKET_DEV: s->st_dev++; break;
    case SOCKET_INO: s->st_ino++; break;
    case SOCKET_NLINK: s->st_nlink++; break;
    case SOCKET_REPLACED: if (source_stats == 2) s->st_ino++; break;
    case SOCKET_AFTER_UID: if (source_stats == 2) s->st_uid++; break;
    case SOCKET_AFTER_MODE: if (source_stats == 2) s->st_mode |= 1; break;
    case SOCKET_AFTER_NLINK: if (source_stats == 2) s->st_nlink++; break;
    default: break;
    }
    return 0;
}

/* Private stand-in layout, intentionally not an SDK ABI qualification. */
#pragma pack(push, 4)
struct socket_reply {
    uint32_t length;
    dev_t device;
    fsobj_type_t type;
    uid_t owner;
    uint32_t mode;
    attrreference_t security;
    uint64_t inode;
};
#pragma pack(pop)

int getattrlistat(int fd, const char *leaf, struct attrlist *r, void *buffer,
    size_t size, unsigned long options)
{
    struct socket_reply reply = {0};
    struct kauth_filesec sec = {0};
    size_t length = KAUTH_FILESEC_SIZE(0);
    socket_attrs++;
    assert(fd == 42 && !strcmp(leaf, "receiver.sock") && source_stats == 1);
    assert(size == sizeof(reply) + 1024 && options == (FSOPT_NOFOLLOW | FSOPT_REPORT_FULLSIZE));
    assert(r->bitmapcount == ATTR_BIT_MAP_COUNT && !r->reserved);
    assert(r->commonattr == (ATTR_CMN_DEVID | ATTR_CMN_OBJTYPE | ATTR_CMN_OWNERID |
        ATTR_CMN_ACCESSMASK | ATTR_CMN_EXTENDED_SECURITY | ATTR_CMN_FILEID));
    assert(!r->volattr && !r->dirattr && !r->fileattr && !r->forkattr);
    switch (socket_scenario) {
    case SOCKET_QUERY_UNSUPPORTED: errno = ENOTSUP; return -1;
    case SOCKET_QUERY_INVALID: errno = EINVAL; return -1;
    case SOCKET_QUERY_MISSING: errno = ENOENT; return -1;
    case SOCKET_QUERY_DENIED: errno = EACCES; return -1;
    default: break;
    }
    sec.fsec_magic = KAUTH_FILESEC_MAGIC;
    if (socket_scenario == SOCKET_NONE) length = 0;
    if (socket_scenario == SOCKET_ACE) { sec.fsec_acl.acl_entrycount = 1; length += sizeof(struct kauth_ace); }
    if (socket_scenario == SOCKET_SENTINEL) sec.fsec_acl.acl_entrycount = KAUTH_FILESEC_NOACL;
    if (socket_scenario == SOCKET_MAGIC) sec.fsec_magic++;
    if (socket_scenario == SOCKET_SHORT_HEADER) length--;
    if (socket_scenario == SOCKET_COUNT_OVERFLOW) sec.fsec_acl.acl_entrycount = UINT32_MAX - 1;
    if (socket_scenario == SOCKET_LENGTH) length++;
    if (socket_scenario == SOCKET_COUNT_MISMATCH) sec.fsec_acl.acl_entrycount = 2;
    reply.length = (uint32_t)(sizeof(reply) + length);
    reply.device = 12;
    reply.type = VSOCK;
    reply.owner = 501;
    reply.mode = socket_mode;
    reply.inode = 34;
    reply.security.attr_dataoffset = sizeof(reply) - offsetof(struct socket_reply, security);
    reply.security.attr_length = (uint32_t)length;
    switch (socket_scenario) {
    case SOCKET_TOTAL_SHORT: reply.length = sizeof(reply) - 1; break;
    case SOCKET_TOTAL_LONG: reply.length = (uint32_t)size + 1; break;
    case SOCKET_OFFSET_NEGATIVE: reply.security.attr_dataoffset = INT32_MIN; break;
    case SOCKET_OFFSET_OVERFLOW: reply.security.attr_dataoffset = INT32_MAX; break;
    case SOCKET_OFFSET_UNALIGNED: reply.security.attr_dataoffset++; break;
    case SOCKET_OFFSET_FIXED: reply.security.attr_dataoffset = 0; break;
    case SOCKET_BLOB_TRUNCATED: reply.length--; break;
    case SOCKET_REF_LENGTH_OVERFLOW: reply.security.attr_length = UINT32_MAX; break;
    case SOCKET_ATTR_TYPE: reply.type++; break;
    case SOCKET_ATTR_UID: reply.owner++; break;
    case SOCKET_ATTR_MODE: reply.mode |= 1; break;
    case SOCKET_ATTR_DEV: reply.device++; break;
    case SOCKET_ATTR_INO: reply.inode++; break;
    default: break;
    }
    memcpy(buffer, &reply, sizeof(reply));
    memcpy((char *)buffer + sizeof(reply), &sec, length);
    return 0;
}

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
    for (socket_scenario = SOCKET_NONE; socket_scenario < SOCKET_COUNT; socket_scenario++) {
        int result, error;
        scenario = ABSENT;
        stats = attrs = gets = entries = frees = 0;
        parent_stats = source_stats = socket_attrs = 0;
        socket_mode = socket_scenario == SOCKET_INITIAL_777 ? 0777 :
            socket_scenario == SOCKET_INITIAL_700 ? 0700 : 0600;
        result = coven_socket_entry_acl_state(42, "receiver.sock", 12, 34, 501, socket_mode);
        error = errno;
        if (socket_scenario == SOCKET_NONE || socket_scenario == SOCKET_ZERO ||
            socket_scenario == SOCKET_SENTINEL || socket_scenario >= SOCKET_INITIAL_777) {
            assert(result == 0 && source_stats == 2 && socket_attrs == 1);
        } else if (socket_scenario == SOCKET_ACE) {
            assert(result == 1 && source_stats == 2);
        } else assert(result == -1);
        if (socket_scenario == SOCKET_QUERY_UNSUPPORTED) assert(error == ENOTSUP);
        if (socket_scenario == SOCKET_QUERY_INVALID ||
            (socket_scenario >= SOCKET_MAGIC && socket_scenario <= SOCKET_OFFSET_FIXED)) assert(error == EINVAL);
        if (socket_scenario == SOCKET_QUERY_MISSING || socket_scenario == SOCKET_BEFORE_ERROR ||
            socket_scenario == SOCKET_AFTER_ERROR) assert(error == ENOENT);
        if (socket_scenario == SOCKET_PARENT_STAT) assert(error == EBADF);
        if (socket_scenario >= SOCKET_SYMLINK && socket_scenario <= SOCKET_NLINK) assert(socket_attrs == 0);
    }
    /* All old guard failures must stop before trusting any socket metadata. */
    for (scenario = ALLOW; scenario < COUNT; scenario++) {
        int result;
        stats = attrs = gets = entries = frees = 0;
        parent_stats = source_stats = socket_attrs = 0;
        result = coven_socket_entry_acl_state(42, "receiver.sock", 12, 34, 501, 0600);
        assert(result != 0 && parent_stats == 0 && source_stats == 0 && socket_attrs == 0);
    }
    printf("macos socket mock logic: %d scenarios + %d parent guard failures passed (not SDK/native qualification)\n",
        SOCKET_COUNT, COUNT - ALLOW);
    return 0;
}
