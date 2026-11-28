package org.yuris.kernel;

import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.os.Build;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * 设备外置视频解码能力探测（设置页展示用）。
 *
 * <p>两条事实合并显示：
 * <ul>
 *   <li>引擎侧：{@link NativeHost#nativeMovieCodecs()} —— 与
 *       {@code movie_backend=external} 实际会尝试的 MIME 表同一份，
 *       由 NDK {@code AMediaCodec_createDecoderByType} 实测；</li>
 *   <li>设备侧：{@link MediaCodecList} —— 补出硬件/软件标记
 *       （{@code isHardwareAccelerated()} 仅 API 29+，低版本按 codec 名回退）。</li>
 * </ul>
 *
 * <p>只读探测，不 configure/start 任何 codec，不影响正在运行的游戏。
 */
final class MovieCodecProbe {
    private MovieCodecProbe() {}

    /** 设置页用的一行摘要（可能含换行）。 */
    static String summary() {
        return I18n.t("引擎外置：%s\n设备解码器：%s", engineSupport(), deviceSupport())
                + "\n"
                + I18n.t("内置 FFmpeg 软解：ASF/WMV(VC-1)、AVI、FLV、MPEG-PS/TS 等系统不支持的格式");
    }

    /** 引擎侧实际能创建的 MIME 列表。 */
    static String engineSupport() {
        try {
            JSONObject root = new JSONObject(NativeHost.nativeMovieCodecs());
            JSONArray codecs = root.optJSONArray("codecs");
            if (codecs == null) {
                return I18n.t("未返回能力清单");
            }
            List<String> names = new ArrayList<>();
            for (int i = 0; i < codecs.length(); i++) {
                JSONObject codec = codecs.optJSONObject(i);
                if (codec != null && codec.optBoolean("decoder", false)) {
                    names.add(label(codec.optString("mime", "")));
                }
            }
            if (names.isEmpty()) {
                return I18n.t("设备没有可用的系统视频解码器");
            }
            // minSdk 24:String.join(Iterable) 要 API 26,手拼。
            StringBuilder out = new StringBuilder();
            for (String name : names) {
                if (out.length() > 0) {
                    out.append(" / ");
                }
                out.append(name);
            }
            return out.toString();
        } catch (Throwable error) {
            // 探测失败不挡启动：外置不可用时 Auto 会回退内置。
            return I18n.t("探测失败（%s）", error.getMessage());
        }
    }

    /** 设备侧视频解码器（按 MIME 去重，标硬件/软件）。 */
    static String deviceSupport() {
        try {
            Map<String, boolean[]> byMime = new LinkedHashMap<>();
            MediaCodecInfo[] infos =
                    new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos();
            for (MediaCodecInfo info : infos) {
                if (info == null || info.isEncoder()) {
                    continue;
                }
                for (String mime : info.getSupportedTypes()) {
                    if (mime == null || !mime.startsWith("video/")) {
                        continue;
                    }
                    try {
                        if (info.getCapabilitiesForType(mime) == null) {
                            continue;
                        }
                    } catch (IllegalArgumentException ignored) {
                        continue;
                    }
                    boolean[] flags = byMime.get(mime);
                    if (flags == null) {
                        flags = new boolean[2];
                        byMime.put(mime, flags);
                    }
                    flags[isHardware(info) ? 0 : 1] = true;
                }
            }
            if (byMime.isEmpty()) {
                return I18n.t("设备未报告视频解码器");
            }
            StringBuilder out = new StringBuilder();
            int shown = 0;
            for (Map.Entry<String, boolean[]> entry : byMime.entrySet()) {
                if (shown >= 8) {
                    out.append(" …");
                    break;
                }
                if (shown > 0) {
                    out.append(" / ");
                }
                boolean[] flags = entry.getValue();
                out.append(label(entry.getKey())).append('(');
                if (flags[0] && flags[1]) {
                    out.append(I18n.t("硬/软"));
                } else if (flags[0]) {
                    out.append(I18n.t("硬"));
                } else if (flags[1]) {
                    out.append(I18n.t("软"));
                }
                out.append(')');
                shown++;
            }
            return out.toString();
        } catch (Throwable error) {
            return I18n.t("探测失败（%s）", error.getMessage());
        }
    }

    /** 硬件解码标记；API 29+ 用官方接口，低版本按 codec 名回退。 */
    private static boolean isHardware(MediaCodecInfo info) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            return info.isHardwareAccelerated();
        }
        String name = info.getName() == null
                ? ""
                : info.getName().toLowerCase(Locale.ROOT);
        // Google/Android 参考实现与显式 software 标记都按软解处理。
        return !(name.startsWith("omx.google.")
                || name.startsWith("c2.android.")
                || name.contains(".sw.")
                || name.contains("software"));
    }

    /** MIME → 用户可读短名。 */
    private static String label(String mime) {
        if (mime == null) {
            return "?";
        }
        switch (mime) {
            case "video/avc":
                return "H.264";
            case "video/hevc":
                return "HEVC";
            case "video/mp4v-es":
                return "MPEG-4";
            case "video/3gpp":
                return "H.263";
            case "video/x-vnd.on2.vp8":
                return "VP8";
            case "video/x-vnd.on2.vp9":
                return "VP9";
            case "video/av01":
                return "AV1";
            case "video/mpeg2":
                return "MPEG-2";
            default:
                return mime;
        }
    }
}
