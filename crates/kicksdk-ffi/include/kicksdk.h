/* kicksdk behind a C ABI. UTF-8 JSON in and out; see src/lib.rs.
 *
 * Every char* returned is an envelope, {"ok": value} or
 * {"error": {"kind", "status", "message"}}, and must be given back to
 * kicksdk_free_string. */
#ifndef KICKSDK_H
#define KICKSDK_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KicksdkSession KicksdkSession;

uint32_t kicksdk_abi_version(void);
char *kicksdk_call(const char *name, const char *args_json);
KicksdkSession *kicksdk_session_new(const char *channels_json);
char *kicksdk_session_call(KicksdkSession *session, const char *method, const char *args_json);
void kicksdk_session_free(KicksdkSession *session);
void kicksdk_free_string(char *s);

#ifdef __cplusplus
}
#endif

#endif
