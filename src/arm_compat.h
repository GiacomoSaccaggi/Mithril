#ifndef MITHRIL_ARM_COMPAT_H
#define MITHRIL_ARM_COMPAT_H

#if defined(__ARM_NEON) && defined(__aarch64__)
#include <arm_neon.h>

#ifndef vld1q_s8_x4
static inline int8x16x4_t mithril_vld1q_s8_x4(const int8_t *p) {
    int8x16x4_t r;
    r.val[0] = vld1q_s8(p);
    r.val[1] = vld1q_s8(p + 16);
    r.val[2] = vld1q_s8(p + 32);
    r.val[3] = vld1q_s8(p + 48);
    return r;
}
#define vld1q_s8_x4 mithril_vld1q_s8_x4
#endif

#ifndef vld1q_s8_x2
static inline int8x16x2_t mithril_vld1q_s8_x2(const int8_t *p) {
    int8x16x2_t r;
    r.val[0] = vld1q_s8(p);
    r.val[1] = vld1q_s8(p + 16);
    return r;
}
#define vld1q_s8_x2 mithril_vld1q_s8_x2
#endif

#ifndef vld1q_u8_x2
static inline uint8x16x2_t mithril_vld1q_u8_x2(const uint8_t *p) {
    uint8x16x2_t r;
    r.val[0] = vld1q_u8(p);
    r.val[1] = vld1q_u8(p + 16);
    return r;
}
#define vld1q_u8_x2 mithril_vld1q_u8_x2
#endif

#ifndef vld1q_u8_x4
static inline uint8x16x4_t mithril_vld1q_u8_x4(const uint8_t *p) {
    uint8x16x4_t r;
    r.val[0] = vld1q_u8(p);
    r.val[1] = vld1q_u8(p + 16);
    r.val[2] = vld1q_u8(p + 32);
    r.val[3] = vld1q_u8(p + 48);
    return r;
}
#define vld1q_u8_x4 mithril_vld1q_u8_x4
#endif

#ifndef vld1q_s16_x2
static inline int16x8x2_t mithril_vld1q_s16_x2(const int16_t *p) {
    int16x8x2_t r;
    r.val[0] = vld1q_s16(p);
    r.val[1] = vld1q_s16(p + 8);
    return r;
}
#define vld1q_s16_x2 mithril_vld1q_s16_x2
#endif

#endif // __ARM_NEON && __aarch64__
#endif // MITHRIL_ARM_COMPAT_H
