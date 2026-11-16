/*
 * yu-host-ios · C ABI(骨架)
 *
 * 线程模型(与 Android 宿主一致):
 *   - 除 yu_ios_suspend / yu_ios_resume / yu_ios_pending_dialogs /
 *     yu_ios_answer_dialog / yu_ios_shutdown 外,所有调用必须在同一条
 *     OS 线程上(推荐专用渲染线程;Swift 骨架先放主线程)。
 *   - yu_ios_probe_root 不碰会话,可在任意线程、init 之前调用。
 *
 * 字符串约定:
 *   - 入参一律 UTF-8、以 NUL 结尾。
 *   - 出参由调用方给缓冲;函数返回「所需字节数(含结尾 NUL)」。
 *     缓冲为 NULL 或 cap 不足时不写,调用方按返回值扩容后重试。
 *     不需要 free。
 *
 * 返回码:
 *   - 0 成功;-1 失败;-2 参数/编码错误;-3 未接线;-99 panic 兜底。
 *   - tick 另有:1 游戏结束 / 4 宿主销毁请求。
 */

#ifndef YU_IOS_H
#define YU_IOS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 版本与 ABI。yu_ios_version 返回静态存储,调用方不释放。 */
const char *yu_ios_version(void);
uint32_t yu_ios_abi_version(void);

/* 会话:JSON 键与 Android nativeInitJson 相同;backend 缺省 soft。 */
int32_t yu_ios_init_json(const char *json);

/* 推进一帧。 */
int32_t yu_ios_tick(void);

/* 当前帧尺寸;(0,0) = 还没有帧。 */
void yu_ios_frame_size(uint32_t *out_w, uint32_t *out_h);

/*
 * 拉取当前帧 RGBA(游戏视口像素,行优先,4 字节/像素)。
 * 返回写入字节数;-1 = 还没有帧;-2 = 缓冲不足或 out 为空。
 */
int64_t yu_ios_copy_frame(uint8_t *out, size_t cap);

/* 输入:button 取 MOUSE_L / MOUSE_R / KEY_ENTER …;坐标是游戏视口像素。 */
int32_t yu_ios_set_input(const char *button, int32_t pressed);
int32_t yu_ios_set_cursor(int64_t x, int64_t y);

/* 生命周期。 */
int32_t yu_ios_suspend(void);
int32_t yu_ios_resume(void);
/* 退出细分:0 运行中 / 1 主脚本走完 / 2 脚本 END / 3 END+RESTART。 */
int32_t yu_ios_lifecycle(void);
/* 1 = 有未取的关窗请求;0 = 没有。 */
int32_t yu_ios_take_shutdown_request(void);
int32_t yu_ios_request_resize(uint32_t width, uint32_t height);
/* 释放会话:0 成功,-1 当前没有会话。 */
int32_t yu_ios_shutdown(void);

/* DIALOG:空串 = 没有待答对话框;id/button 与 JSON 里的字段一致。 */
int32_t yu_ios_pending_dialogs(char *out, size_t cap);
int32_t yu_ios_answer_dialog(uint64_t id, uint8_t button, const char *text);

/* 诊断。 */
int32_t yu_ios_status(char *out, size_t cap);
int32_t yu_ios_list_fonts(char *out, size_t cap);
int32_t yu_ios_probe_root(const char *path, char *out, size_t cap);
int32_t yu_ios_last_error(char *out, size_t cap);

/*
 * Metal 上屏(预留)。
 * layer = CAMetalLayer*;当前固定返回 -3(未接线),soft 模式不要调用。
 */
int32_t yu_ios_attach_layer(void *layer, uint32_t width, uint32_t height);

#ifdef __cplusplus
}
#endif

#endif /* YU_IOS_H */
