/* kicksdk behind a C ABI. UTF-8 JSON in and out; see src/lib.rs.
 *
 * Every char* returned is an envelope, {"ok": value} or
 * {"error": {"kind", "status", "message"}}, never NULL, and must be given
 * back to kicksdk_free_string. A panic inside comes back as an envelope of
 * kind "panic"; it is not printed to stderr.
 *
 * Check kicksdk_abi_version() == KICKSDK_ABI_VERSION after loading the
 * library: a different number means this header does not describe it. */
#ifndef KICKSDK_H
#define KICKSDK_H

#include <stdint.h>

/* What kicksdk_abi_version() returns for the library this header describes. */
#define KICKSDK_ABI_VERSION 1u

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KicksdkSession KicksdkSession;

uint32_t kicksdk_abi_version(void);
char *kicksdk_call(const char *name, const char *args_json);
/* NULL when channels_json is not a JSON list of strings: check it. */
KicksdkSession *kicksdk_session_new(const char *channels_json);
/* A session is not thread-safe: one call at a time per session. */
char *kicksdk_session_call(KicksdkSession *session, const char *method, const char *args_json);
void kicksdk_session_free(KicksdkSession *session);
void kicksdk_free_string(char *s);

#ifdef __cplusplus
}
#endif

#endif
