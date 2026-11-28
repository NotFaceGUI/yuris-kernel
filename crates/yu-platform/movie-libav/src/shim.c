/*
 * yu-movie-libav · FFmpeg C 侧薄封装
 *
 * 目的:让 Rust 侧完全不接触 FFmpeg 的结构体布局(AVFrame/AVPacket 会随
 * 大版本变),只通过不透明的 YeMovie* 和定长错误串交互。所有解复用/解码/
 * 像素与采样格式转换都在这里完成:
 *
 *   - 容器/码流:libavformat(libavcodec 的 send/receive 新 API);
 *   - 视频:解码帧 → swscale → RGBA8(行优先,alpha=255);
 *   - 音频:解码帧 → swresample → 交错 s16 立体声。
 *
 * 时间轴:视频 PTS 以「首个视频帧」归零(与内置 pl_mpeg / Android
 * MediaCodec 后端同一口径)。音频不参与内核时钟。
 */

#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
#include <libavutil/channel_layout.h>
#include <libavutil/cpu.h>
#include <libavutil/error.h>
#include <libavutil/imgutils.h>
#include <libavutil/opt.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>

#define YE_ERR_LEN 256

typedef struct YeMovie {
    AVFormatContext *fmt;
    int video_stream;
    int audio_stream;

    AVCodecContext *vctx;
    AVCodecContext *actx;

    AVRational vtb;
    AVRational atb;

    double video_base;
    int have_video_base;
    int64_t video_frames;

    struct SwsContext *sws;
    int sws_w;
    int sws_h;
    int sws_fmt;

    struct SwrContext *swr;
    int out_rate;
    int out_channels;

    AVPacket *pending;
    int pending_stream;
    int pending_valid;

    int demux_eof;
    int flush_sent;
    int video_eof;
    int audio_eof;

    int width;
    int height;
    double fps;
    double duration;

    int64_t send_errors;
} YeMovie;

void ye_movie_close(YeMovie *m);

static void ye_set_err(char *err, int err_len, const char *what, int code)
{
    if (!err || err_len <= 0) {
        return;
    }
    char detail[AV_ERROR_MAX_STRING_SIZE] = {0};
    if (code != 0) {
        av_strerror(code, detail, sizeof(detail));
        snprintf(err, (size_t)err_len, "%s: %s", what, detail);
    } else {
        snprintf(err, (size_t)err_len, "%s", what);
    }
    err[err_len - 1] = '\0';
}

const char *ye_movie_version(void)
{
    return av_version_info();
}

static int ye_open_codec(AVFormatContext *fmt, int stream_index, AVCodecContext **out)
{
    AVStream *stream = fmt->streams[stream_index];
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (!codec) {
        return AVERROR_DECODER_NOT_FOUND;
    }
    AVCodecContext *ctx = avcodec_alloc_context3(codec);
    if (!ctx) {
        return AVERROR(ENOMEM);
    }
    int ret = avcodec_parameters_to_context(ctx, stream->codecpar);
    if (ret < 0) {
        avcodec_free_context(&ctx);
        return ret;
    }
    ctx->pkt_timebase = stream->time_base;
    // 解码线程对 VC-1/H.264 都有收益,但游戏渲染/音频也要 CPU:封顶 4。
    int threads = av_cpu_count();
    if (threads < 1) {
        threads = 1;
    }
    if (threads > 4) {
        threads = 4;
    }
    ctx->thread_count = threads;
    ret = avcodec_open2(ctx, codec, NULL);
    if (ret < 0) {
        avcodec_free_context(&ctx);
        return ret;
    }
    *out = ctx;
    return 0;
}

YeMovie *ye_movie_open(const char *path, char *err, int err_len)
{
    if (!path) {
        ye_set_err(err, err_len, "影片路径为空", 0);
        return NULL;
    }
    YeMovie *m = av_mallocz(sizeof(*m));
    if (!m) {
        ye_set_err(err, err_len, "内存不足", 0);
        return NULL;
    }
    m->video_stream = -1;
    m->audio_stream = -1;
    m->pending_stream = -1;

    int ret = avformat_open_input(&m->fmt, path, NULL, NULL);
    if (ret < 0) {
        ye_set_err(err, err_len, "打开影片容器失败", ret);
        av_free(m);
        return NULL;
    }
    ret = avformat_find_stream_info(m->fmt, NULL);
    if (ret < 0) {
        ye_set_err(err, err_len, "读取影片流信息失败", ret);
        ye_movie_close(m);
        return NULL;
    }

    const AVCodec *vdec = NULL;
    m->video_stream = av_find_best_stream(m->fmt, AVMEDIA_TYPE_VIDEO, -1, -1, &vdec, 0);
    if (m->video_stream < 0) {
        ye_set_err(err, err_len, "影片没有视频轨", 0);
        ye_movie_close(m);
        return NULL;
    }
    ret = ye_open_codec(m->fmt, m->video_stream, &m->vctx);
    if (ret < 0) {
        ye_set_err(err, err_len, "打开视频解码器失败", ret);
        ye_movie_close(m);
        return NULL;
    }
    AVStream *vstream = m->fmt->streams[m->video_stream];
    m->vtb = vstream->time_base;
    m->width = vstream->codecpar->width;
    m->height = vstream->codecpar->height;
    if (m->width <= 0 || m->height <= 0) {
        m->width = m->vctx->width;
        m->height = m->vctx->height;
    }
    if (m->width <= 0 || m->height <= 0) {
        ye_set_err(err, err_len, "视频轨缺少宽高", 0);
        ye_movie_close(m);
        return NULL;
    }
    AVRational fr = av_guess_frame_rate(m->fmt, vstream, NULL);
    m->fps = (fr.num > 0 && fr.den > 0) ? av_q2d(fr) : 30.0;
    m->duration = m->fmt->duration > 0 ? (double)m->fmt->duration / (double)AV_TIME_BASE : 0.0;

    m->audio_stream = av_find_best_stream(m->fmt, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0);
    if (m->audio_stream >= 0) {
        ret = ye_open_codec(m->fmt, m->audio_stream, &m->actx);
        if (ret < 0) {
            // 音频失败不致命:视频照播(与 Android 后端同一取舍)。
            m->audio_stream = -1;
        }
    }
    if (m->actx) {
        AVStream *astream = m->fmt->streams[m->audio_stream];
        m->atb = astream->time_base;
        m->out_rate = m->actx->sample_rate > 0 ? m->actx->sample_rate : 48000;
        m->out_channels = 2;
        AVChannelLayout in_layout;
        if (av_channel_layout_check(&m->actx->ch_layout)) {
            av_channel_layout_copy(&in_layout, &m->actx->ch_layout);
        } else {
            av_channel_layout_default(&in_layout, m->actx->ch_layout.nb_channels > 0
                                                      ? m->actx->ch_layout.nb_channels
                                                      : 2);
        }
        AVChannelLayout out_layout = AV_CHANNEL_LAYOUT_STEREO;
        ret = swr_alloc_set_opts2(&m->swr, &out_layout, AV_SAMPLE_FMT_S16, m->out_rate,
                                  &in_layout, m->actx->sample_fmt, m->actx->sample_rate, 0, NULL);
        av_channel_layout_uninit(&in_layout);
        if (ret < 0 || !m->swr || swr_init(m->swr) < 0) {
            swr_free(&m->swr);
            avcodec_free_context(&m->actx);
            m->audio_stream = -1;
        }
    }
    m->pending = av_packet_alloc();
    if (!m->pending) {
        ye_set_err(err, err_len, "内存不足(影片包缓冲)", 0);
        ye_movie_close(m);
        return NULL;
    }
    return m;
}

void ye_movie_close(YeMovie *m)
{
    if (!m) {
        return;
    }
    if (m->pending) {
        av_packet_free(&m->pending);
    }
    sws_freeContext(m->sws);
    swr_free(&m->swr);
    avcodec_free_context(&m->vctx);
    avcodec_free_context(&m->actx);
    if (m->fmt) {
        avformat_close_input(&m->fmt);
    }
    av_free(m);
}

static int ye_send(YeMovie *m, AVPacket *pkt, int stream)
{
    if (stream == m->video_stream && m->vctx) {
        return avcodec_send_packet(m->vctx, pkt);
    }
    if (stream == m->audio_stream && m->actx) {
        return avcodec_send_packet(m->actx, pkt);
    }
    return 0;
}

static void ye_flush_codecs(YeMovie *m)
{
    if (m->flush_sent) {
        return;
    }
    m->flush_sent = 1;
    if (m->vctx) {
        avcodec_send_packet(m->vctx, NULL);
    }
    if (m->actx) {
        avcodec_send_packet(m->actx, NULL);
    }
}

/*
 * 读一个包并交给对应解码器。
 * 返回:1 = 有进展;0 = 解码器暂时不收(调用方先排空输出);-1 = 解复用结束;
 *       -2 = 致命错误。
 */
int ye_movie_feed(YeMovie *m)
{
    if (!m) {
        return -2;
    }
    if (m->demux_eof) {
        return -1;
    }
    if (m->pending_valid) {
        int ret = ye_send(m, m->pending, m->pending_stream);
        if (ret == AVERROR(EAGAIN)) {
            return 0;
        }
        av_packet_unref(m->pending);
        m->pending_valid = 0;
        if (ret < 0) {
            m->send_errors++;
        }
        return 1;
    }

    AVPacket *pkt = av_packet_alloc();
    if (!pkt) {
        return -2;
    }
    int ret = av_read_frame(m->fmt, pkt);
    if (ret == AVERROR_EOF) {
        m->demux_eof = 1;
        ye_flush_codecs(m);
        av_packet_free(&pkt);
        return -1;
    }
    if (ret < 0) {
        av_packet_free(&pkt);
        return -2;
    }
    int stream = pkt->stream_index;
    if (stream != m->video_stream && stream != m->audio_stream) {
        av_packet_free(&pkt);
        return 1;
    }
    ret = ye_send(m, pkt, stream);
    if (ret == AVERROR(EAGAIN)) {
        // 解码器输出未排空:把包挂起,下一 tick 重试(不能丢)。
        av_packet_move_ref(m->pending, pkt);
        m->pending_stream = stream;
        m->pending_valid = 1;
        av_packet_free(&pkt);
        return 0;
    }
    if (ret < 0) {
        // 单个坏包不至于让整片失败;记数后继续。
        m->send_errors++;
    }
    av_packet_free(&pkt);
    return 1;
}

/*
 * 取一帧视频并转成 RGBA8。
 * 返回:1 = 产出帧(pts/宽高已写);0 = 暂无输出;-1 = 视频结束;
 *       -2 = 错误;-3 = rgba 容量不足(宽高已写,调用方扩容后重试)。
 */
int ye_movie_video(YeMovie *m, unsigned char *rgba, int rgba_cap,
                   double *out_pts, int *out_w, int *out_h)
{
    if (!m || m->video_stream < 0 || !m->vctx) {
        return -1;
    }
    if (m->video_eof) {
        return -1;
    }
    AVFrame *frame = av_frame_alloc();
    if (!frame) {
        return -2;
    }
    int ret = avcodec_receive_frame(m->vctx, frame);
    if (ret == AVERROR(EAGAIN)) {
        av_frame_free(&frame);
        return 0;
    }
    if (ret == AVERROR_EOF) {
        m->video_eof = 1;
        av_frame_free(&frame);
        return -1;
    }
    if (ret < 0) {
        av_frame_free(&frame);
        return -2;
    }

    int w = frame->width;
    int h = frame->height;
    if (w <= 0 || h <= 0) {
        av_frame_free(&frame);
        return 0;
    }
    int64_t need = (int64_t)w * (int64_t)h * 4;
    if (out_w) {
        *out_w = w;
    }
    if (out_h) {
        *out_h = h;
    }
    if (!rgba || need > rgba_cap) {
        av_frame_free(&frame);
        return -3;
    }

    double pts = 0.0;
    int64_t ts = frame->best_effort_timestamp;
    if (ts == AV_NOPTS_VALUE) {
        ts = frame->pts;
    }
    if (ts != AV_NOPTS_VALUE) {
        pts = (double)ts * av_q2d(m->vtb);
        if (!m->have_video_base) {
            m->video_base = pts;
            m->have_video_base = 1;
        }
        pts -= m->video_base;
        if (pts < 0.0) {
            pts = 0.0;
        }
    } else {
        pts = (double)m->video_frames / (m->fps > 0.0 ? m->fps : 30.0);
    }
    m->video_frames++;

    if (!m->sws || m->sws_w != w || m->sws_h != h || m->sws_fmt != (int)frame->format) {
        sws_freeContext(m->sws);
        m->sws = sws_getContext(w, h, (enum AVPixelFormat)frame->format, w, h,
                                AV_PIX_FMT_RGBA, SWS_POINT, NULL, NULL, NULL);
        m->sws_w = w;
        m->sws_h = h;
        m->sws_fmt = (int)frame->format;
        if (!m->sws) {
            av_frame_free(&frame);
            return -2;
        }
    }
    uint8_t *dst_data[4] = {rgba, NULL, NULL, NULL};
    int dst_linesize[4] = {w * 4, 0, 0, 0};
    sws_scale(m->sws, (const uint8_t *const *)frame->data, frame->linesize, 0, h,
              dst_data, dst_linesize);
    if (out_pts) {
        *out_pts = pts;
    }
    av_frame_free(&frame);
    return 1;
}

/*
 * 取一帧音频并转成交错 s16 立体声。
 * 返回:>0 = 写入的 i16 个数;0 = 暂无输出;-1 = 音频结束;-2 = 错误。
 */
int ye_movie_audio(YeMovie *m, short *out, int cap_i16, double *out_pts)
{
    if (!m || m->audio_stream < 0 || !m->actx || !m->swr) {
        return -1;
    }
    if (m->audio_eof) {
        return -1;
    }
    AVFrame *frame = av_frame_alloc();
    if (!frame) {
        return -2;
    }
    int ret = avcodec_receive_frame(m->actx, frame);
    if (ret == AVERROR(EAGAIN)) {
        av_frame_free(&frame);
        return 0;
    }
    if (ret == AVERROR_EOF) {
        m->audio_eof = 1;
        av_frame_free(&frame);
        return -1;
    }
    if (ret < 0) {
        av_frame_free(&frame);
        return -2;
    }

    int max_out = swr_get_out_samples(m->swr, frame->nb_samples);
    if (max_out < 0) {
        av_frame_free(&frame);
        return -2;
    }
    int max_i16 = max_out * m->out_channels;
    // cap_i16 由 Rust 侧给足(WMA 一包 ≤ 2048 帧 × 2);超出说明调用方
    // 缓冲太小,直接丢弃本帧,避免跨调用残留半帧。
    if (max_i16 > cap_i16) {
        av_frame_free(&frame);
        return -2;
    }
    uint8_t *dst[1] = {(uint8_t *)out};
    int got = swr_convert(m->swr, dst, max_out,
                          (const uint8_t *const *)frame->extended_data, frame->nb_samples);
    if (got < 0) {
        av_frame_free(&frame);
        return -2;
    }
    if (out_pts) {
        int64_t ts = frame->best_effort_timestamp;
        *out_pts = ts == AV_NOPTS_VALUE ? 0.0 : (double)ts * av_q2d(m->atb);
    }
    av_frame_free(&frame);
    return got * m->out_channels;
}

int ye_movie_info(const YeMovie *m, int *width, int *height, double *fps,
                  int *sample_rate, int *channels, double *duration)
{
    if (!m) {
        return -1;
    }
    if (width) {
        *width = m->width;
    }
    if (height) {
        *height = m->height;
    }
    if (fps) {
        *fps = m->fps;
    }
    if (sample_rate) {
        *sample_rate = m->actx ? m->out_rate : 0;
    }
    if (channels) {
        *channels = m->actx ? m->out_channels : 0;
    }
    if (duration) {
        *duration = m->duration;
    }
    return 0;
}

int ye_movie_video_eof(const YeMovie *m)
{
    return m ? m->video_eof : 1;
}

int64_t ye_movie_send_errors(const YeMovie *m)
{
    return m ? m->send_errors : 0;
}
