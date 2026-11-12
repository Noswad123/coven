#ifndef COVEN_HOOK_ACL_MOCK_H
#define COVEN_HOOK_ACL_MOCK_H
#define _GNU_SOURCE 1
/* Logic fixture only: these deliberately private stand-ins are NOT Apple ABI. */
#ifndef COVEN_HOOK_ACL_TEST
#error mock header is test-only
#endif
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <fcntl.h>
typedef uint32_t fsobj_type_t;
typedef struct { int32_t attr_dataoffset; uint32_t attr_length; } attrreference_t;
struct kauth_ace { unsigned char applicable[16]; uint32_t flags, rights; };
struct kauth_acl { uint32_t acl_entrycount, acl_flags; struct kauth_ace acl_ace[1]; };
struct kauth_filesec {
    uint32_t fsec_magic;
    unsigned char owner[16], group[16];
    struct kauth_acl fsec_acl;
};
#define KAUTH_FILESEC_MAGIC UINT32_C(0x012cc16d)
#define KAUTH_FILESEC_NOACL UINT32_MAX
#define KAUTH_ACL_MAX_ENTRIES 128
#define KAUTH_FILESEC_SIZE(c) (offsetof(struct kauth_filesec, fsec_acl) + offsetof(struct kauth_acl, acl_ace) + (c) * sizeof(struct kauth_ace))
#define VSOCK 6
#define ATTR_CMN_DEVID UINT32_C(0x2)
#define ATTR_CMN_OBJTYPE UINT32_C(0x8)
#define ATTR_CMN_OWNERID UINT32_C(0x8000)
#define ATTR_CMN_ACCESSMASK UINT32_C(0x20000)
#define ATTR_CMN_EXTENDED_SECURITY UINT32_C(0x400000)
#define ATTR_CMN_FILEID UINT32_C(0x2000000)
#define FSOPT_NOFOLLOW 1
#define FSOPT_REPORT_FULLSIZE 4
typedef uint32_t attrgroup_t;
typedef struct { attrgroup_t commonattr, volattr, dirattr, fileattr, forkattr; } attribute_set_t;
typedef struct { uint32_t capabilities[4], valid[4]; } vol_capabilities_attr_t;
struct attrlist {
    uint16_t bitmapcount, reserved;
    attrgroup_t commonattr, volattr, dirattr, fileattr, forkattr;
};
struct statfs { unsigned long f_flags; char f_fstypename[16]; };
typedef struct mock_acl *acl_t;
typedef void *acl_entry_t;
/* Intentionally distinct mock selectors prevent accidental literal reliance. */
#define ACL_TYPE_EXTENDED 71
#define ACL_FIRST_ENTRY 73
#define MNT_LOCAL 0x40
/* Pinned XNU mount.h facts; native code obtains these from the SDK. */
#define MNT_IGNORE_OWNERSHIP UINT32_C(0x00200000)
#define MNT_UNKNOWNPERMISSIONS MNT_IGNORE_OWNERSHIP
#define ATTR_BIT_MAP_COUNT 5
#define ATTR_CMN_RETURNED_ATTRS UINT32_C(0x80000000)
#define ATTR_VOL_INFO UINT32_C(0x80000000)
#define ATTR_VOL_CAPABILITIES UINT32_C(0x20000)
#define VOL_CAPABILITIES_INTERFACES 1
#define VOL_CAP_INT_EXTENDED_SECURITY UINT32_C(0x400)
int fstatfs(int, struct statfs *);
int fgetattrlist(int, struct attrlist *, void *, size_t, unsigned long);
acl_t acl_get_fd_np(int, int);
int acl_get_entry(acl_t, int, acl_entry_t *);
int acl_free(void *);
int coven_hook_acl_state(int);
int getattrlistat(int, const char *, struct attrlist *, void *, size_t, unsigned long);
int coven_socket_entry_acl_state(int, const char *, uint64_t, uint64_t, uint32_t, uint32_t);
#endif
