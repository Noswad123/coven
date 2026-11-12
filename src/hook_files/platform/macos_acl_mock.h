#ifndef COVEN_HOOK_ACL_MOCK_H
#define COVEN_HOOK_ACL_MOCK_H
/* Logic fixture only: these deliberately private stand-ins are NOT Apple ABI. */
#ifndef COVEN_HOOK_ACL_TEST
#error mock header is test-only
#endif
#include <stddef.h>
#include <stdint.h>
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
#endif
