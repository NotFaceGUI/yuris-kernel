package org.yuris.kernel;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.DialogInterface;
import android.os.Handler;
import android.os.Looper;
import android.text.InputFilter;
import android.text.InputType;
import android.text.TextUtils;
import android.util.Log;
import android.view.KeyEvent;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.view.inputmethod.EditorInfo;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.Toast;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * 内核 DIALOG 的<b>壳侧呈现器</b>（本壳的参考实现，可整体替换）。
 *
 * <p>内核只持有事实：{@link NativeHost#nativePendingDialogs()} 给出
 * {@code {"protocol":1,"dialogs":[...]}} 的 JSON（空串 = 没有），
 * {@link NativeHost#nativeAnswerDialog(long, int, String)} 回答。**任何
 * Android 壳都可以用同一份 JSON 自己画**（Compose / 游戏画布内绘制 /
 * 系统对话框），只要遵守三个约定：
 *
 * <ul>
 *   <li>{@code buttons} 是 1 基语义角色（ok/cancel/yes/no），文案由壳
 *       本地化；回答时回传的是<b>序号</b>（1..N），不是角色名；</li>
 *   <li>{@code input=true} 时必须把文本一起交回（编码失败会返回 ERR，
 *       壳应保持输入框打开让用户修改）；</li>
 *   <li>一次只呈现一个（官方 MessageBox 是模态的）；多个待答时排队。</li>
 * </ul>
 *
 * <p>本实现用平台 {@link AlertDialog} + 显式浅色对话框主题：{@code MainActivity}
 * 用的是 {@code Theme.Black.NoTitleBar.Fullscreen}，不能直接用 AppCompat
 * 的 Builder（缺 AppCompat 主题会崩），也不引入 Material 依赖。
 */
final class DialogPresenter {
    static final String TAG = "yu-dialog";
    /** JSON 契约版本；未知版本只记日志，不猜字段。 */
    private static final int PROTOCOL = 1;

    private final Activity activity;
    private final Handler main = new Handler(Looper.getMainLooper());
    /** 当前快照（按 JSON 顺序）。 */
    private final Map<Long, Spec> pending = new LinkedHashMap<>();
    /** 尚未呈现的队列（当前框不在其中）。 */
    private final ArrayDeque<Spec> order = new ArrayDeque<>();
    /** 已成功回答的 id：快照尚未刷新的那一小段时间里不再重复弹。 */
    private final Set<Long> answered = new HashSet<>();
    private Spec current;
    private AlertDialog dialog;
    private boolean destroyed;

    DialogPresenter(Activity activity) {
        this.activity = activity;
    }

    /**
     * 发布最新快照（渲染线程调用；内部转到 UI 线程）。
     *
     * @param json 内核返回的 JSON；空串 = 没有待答对话框。
     */
    void publish(String json) {
        final String snapshot = json == null ? "" : json;
        main.post(() -> apply(snapshot));
    }

    /** Activity 销毁：关掉当前对话框并丢弃队列（可在任意线程调用）。 */
    void dismissAll() {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            dismissNow();
        } else {
            main.post(this::dismissNow);
        }
    }

    private void dismissNow() {
        destroyed = true;
        dismissCurrent();
        pending.clear();
        order.clear();
        answered.clear();
    }

    // ------------------------------------------------------------ UI 线程

    private void apply(String json) {
        if (destroyed || activity.isFinishing() || activity.isDestroyed()) {
            return;
        }
        Map<Long, Spec> next = new LinkedHashMap<>();
        if (!json.isEmpty()) {
            try {
                JSONObject root = new JSONObject(json);
                int protocol = root.optInt("protocol", 0);
                if (protocol != PROTOCOL) {
                    Log.w(TAG, "未知 DIALOG 协议 " + protocol + "，忽略本次快照");
                    return;
                }
                JSONArray dialogs = root.optJSONArray("dialogs");
                if (dialogs != null) {
                    for (int i = 0; i < dialogs.length(); i++) {
                        JSONObject obj = dialogs.optJSONObject(i);
                        if (obj == null) {
                            continue;
                        }
                        Spec spec = Spec.from(obj);
                        if (spec == null || answered.contains(spec.id)) {
                            continue;
                        }
                        next.put(spec.id, spec);
                    }
                }
            } catch (JSONException error) {
                // 生成端固定是合法 JSON；解析失败说明契约实现有 bug，
                // 记日志即可，不能因此让游戏输入永远被锁。
                Log.e(TAG, "DIALOG JSON 解析失败: " + error + " json=" + json);
                return;
            }
        }

        pending.clear();
        pending.putAll(next);
        // 已回答的 id 从快照消失后即可忘掉（DIALOG id 单调递增，
        // 但长会话里没必要无限累积）。
        answered.retainAll(next.keySet());

        // 当前框已不在快照里（被回答 / 游戏结束 / 会话重建）⇒ 关掉。
        if (current != null && !pending.containsKey(current.id)) {
            dismissCurrent();
        }
        rebuildOrder();
        if (current == null) {
            showNext();
        }
    }

    private void rebuildOrder() {
        order.clear();
        for (Spec spec : pending.values()) {
            if (current == null || spec.id != current.id) {
                order.add(spec);
            }
        }
    }

    private void showNext() {
        if (destroyed || current != null) {
            return;
        }
        while (!order.isEmpty()) {
            Spec spec = order.poll();
            if (spec == null || answered.contains(spec.id) || !pending.containsKey(spec.id)) {
                continue;
            }
            show(spec);
            return;
        }
    }

    private void show(Spec spec) {
        if (activity.isFinishing() || activity.isDestroyed()) {
            return;
        }
        AlertDialog.Builder builder = new AlertDialog.Builder(
                activity, android.R.style.Theme_DeviceDefault_Light_Dialog_Alert);
        if (!spec.caption.isEmpty()) {
            builder.setTitle(spec.caption);
        }
        if (!spec.text.isEmpty()) {
            builder.setMessage(spec.text);
        }
        int icon = iconRes(spec.icon);
        if (icon != 0) {
            builder.setIcon(icon);
        }

        // 按钮必须先用 builder 建出来（AlertDialog 只在设置了文案时才创建
        // 对应按钮）；真正的点击处理在 show 之后覆盖，以便 ERR 时保持打开。
        if (spec.roles.length >= 1) {
            builder.setPositiveButton(label(spec.roles[0]), null);
        }
        if (spec.roles.length >= 2) {
            builder.setNegativeButton(label(spec.roles[1]), null);
        }
        if (spec.roles.length >= 3) {
            builder.setNeutralButton(label(spec.roles[2]), null);
        }

        EditText inputBox = null;
        if (spec.input) {
            inputBox = new EditText(activity);
            inputBox.setSingleLine(true);
            inputBox.setInputType(InputType.TYPE_CLASS_TEXT);
            if (spec.inputLimit > 0) {
                inputBox.setFilters(new InputFilter[] {
                        new InputFilter.LengthFilter(spec.inputLimit)
                });
            }
            inputBox.setText(spec.defaultInput);
            inputBox.setSelection(inputBox.getText().length());
            inputBox.setSelectAllOnFocus(true);
            inputBox.setImeOptions(EditorInfo.IME_ACTION_DONE);
            FrameLayout container = new FrameLayout(activity);
            int pad = Math.round(20 * activity.getResources().getDisplayMetrics().density);
            container.setPadding(pad, 0, pad, 0);
            container.addView(inputBox, new FrameLayout.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT));
            builder.setView(container);
        }
        final EditText input = inputBox;

        final AlertDialog created = builder.create();
        created.setCanceledOnTouchOutside(false);
        // 官方语义：Esc/返回键只对带 Cancel 的组合生效；MB_OK / MB_YESNO
        // 不响应取消（`yu_rt/src/dialog.rs` 的按钮序列表）。
        final int cancelIndex = cancelIndex(spec.roles);
        created.setCancelable(cancelIndex != 0);
        created.setOnCancelListener((DialogInterface ignored) -> {
            if (cancelIndex == 0) {
                return;
            }
            boolean accepted = answer(spec, cancelIndex, input);
            // Dialog.cancel() 在回调返回后必然 dismiss：成功时 answer 已经
            // 清理；失败（ERR）时这里必须清状态并重新呈现，否则快照没变
            // ⇒ 壳不会再发布 ⇒ 游戏一直卡在“对话框待答”的输入抑制里。
            if (!accepted) {
                dismissCurrent();
                main.post(() -> {
                    if (!destroyed && current == null && pending.containsKey(spec.id)) {
                        show(spec);
                    }
                });
            } else if (current != null && current.id == spec.id) {
                // answer 因“不是当前框”提前返回等边界：清掉幽灵状态。
                dismissCurrent();
            }
        });
        if (input != null) {
            // 必须赶在 show 之前设，软键盘才会随对话框弹出。
            created.getWindow().setSoftInputMode(
                    WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE
                            | WindowManager.LayoutParams.SOFT_INPUT_STATE_ALWAYS_VISIBLE);
        }
        created.setOnShowListener((DialogInterface ignored) -> {
            bindButtons(created, spec, input);
            if (input != null) {
                input.requestFocus();
                input.setOnEditorActionListener((view, actionId, event) -> {
                    boolean done = actionId == EditorInfo.IME_ACTION_DONE
                            || (event != null
                                && event.getKeyCode() == KeyEvent.KEYCODE_ENTER
                                && event.getAction() == KeyEvent.ACTION_DOWN);
                    if (done) {
                        answer(spec, 1, input);
                        return true;
                    }
                    return false;
                });
            } else if (spec.defaultButton >= 1
                    && spec.defaultButton <= spec.roles.length) {
                Button defaultButton = created.getButton(whichButton(spec.defaultButton));
                if (defaultButton != null) {
                    defaultButton.requestFocus();
                }
            }
        });
        // 硬件键盘：默认键在对话框层也响应 Enter（按钮自身聚焦时不会
        // 重复触发，Dialog 的 OnKeyListener 只收未被 View 消费的键）。
        created.setOnKeyListener((DialogInterface ignored, int keyCode, KeyEvent event) -> {
            if (input == null
                    && keyCode == KeyEvent.KEYCODE_ENTER
                    && event.getAction() == KeyEvent.ACTION_DOWN
                    && spec.defaultButton >= 1
                    && spec.defaultButton <= spec.roles.length) {
                Button defaultButton = created.getButton(whichButton(spec.defaultButton));
                if (defaultButton != null) {
                    defaultButton.performClick();
                    return true;
                }
            }
            return false;
        });
        // 先登记再 show：OnShowListener 里 bindButtons 之后若立刻触发回答
        // （无障碍/硬件键盘），answer() 必须能看到 current。
        current = spec;
        dialog = created;
        try {
            created.show();
        } catch (RuntimeException error) {
            // Activity 正在收尾时窗口 token 可能已失效（BadTokenException）；
            // 不让它冒到 UI 线程崩进程。若 Activity 还活着，半秒后重试；
            // 收尾中的话 onDestroy 会置 destroyed，重试自动放弃。
            current = null;
            dialog = null;
            Log.w(TAG, "DIALOG#" + spec.id + " 无法呈现: " + error);
            if (!destroyed && !activity.isFinishing() && !activity.isDestroyed()) {
                main.postDelayed(() -> {
                    if (!destroyed && current == null && pending.containsKey(spec.id)) {
                        show(spec);
                    }
                }, 500);
            }
            return;
        }
        Log.i(TAG, "DIALOG#" + spec.id + " 呈现 buttons=" + TextUtils.join(",", spec.roles)
                + " input=" + spec.input + " caption=" + spec.caption);
    }

    private void bindButtons(AlertDialog created, Spec spec, EditText input) {
        int count = Math.min(spec.roles.length, 3);
        for (int index = 1; index <= count; index++) {
            Button button = created.getButton(whichButton(index));
            if (button == null) {
                continue;
            }
            final int answerIndex = index;
            button.setOnClickListener(view -> answer(spec, answerIndex, input));
        }
    }

    /**
     * 回答并关闭；{@code ERR:} 时返回 {@code false}（输入不可映射 CP932、
     * 按钮越界、快照过期都会走这里）。
     *
     * <p>普通按钮点击时对话框不会自动关，ERR 保持打开即可；但返回键取消
     * 会被平台在回调后强制 dismiss，取消方据此决定是否重新呈现。
     *
     * @return {@code true} = 已接受/无需处理；{@code false} = 内核拒绝
     */
    private boolean answer(Spec spec, int button, EditText input) {
        if (current == null || current.id != spec.id || answered.contains(spec.id)) {
            return true;
        }
        String text = input == null ? null : input.getText().toString();
        String result;
        try {
            result = NativeHost.nativeAnswerDialog(spec.id, button, text);
        } catch (Throwable error) {
            Log.e(TAG, "nativeAnswerDialog 调用失败", error);
            return false;
        }
        if (result != null && result.startsWith("ERR")) {
            Log.w(TAG, "DIALOG#" + spec.id + " 回答被拒: " + result);
            Toast.makeText(activity, result, Toast.LENGTH_LONG).show();
            return false;
        }
        if (result == null || result.isEmpty()) {
            // 只在 JNI 字符串分配失败时出现；回答本身可能已入队，按成功处理。
            Log.w(TAG, "DIALOG#" + spec.id + " nativeAnswerDialog 返回空串");
        }
        answered.add(spec.id);
        pending.remove(spec.id);
        dismissCurrent();
        Log.i(TAG, "DIALOG#" + spec.id + " answered button=" + button);
        // 下一次 show 放到 UI 循环下一轮，避免在 dismiss 回调里立刻建新窗。
        main.post(this::showNext);
        return true;
    }

    private void dismissCurrent() {
        current = null;
        AlertDialog closing = dialog;
        dialog = null;
        if (closing != null) {
            try {
                closing.dismiss();
            } catch (IllegalArgumentException ignored) {
                // Activity 已销毁时窗口 token 可能失效；无需再处理。
            }
        }
    }

    // ------------------------------------------------------------ 映射表

    /** 1 基序号 → AlertDialog 按钮位。 */
    private static int whichButton(int index) {
        switch (index) {
            case 1:
                return DialogInterface.BUTTON_POSITIVE;
            case 2:
                return DialogInterface.BUTTON_NEGATIVE;
            case 3:
                return DialogInterface.BUTTON_NEUTRAL;
            default:
                return DialogInterface.BUTTON_POSITIVE;
        }
    }

    /**
     * 按钮文案。这是<b>壳层</b>的选择：官方 MessageBox 在日文 Windows 上
     * 用这套文案，桌面 egui 也照此；换成系统语言/中文只需改这里。
     */
    private static String label(String role) {
        switch (role) {
            case "ok":
                return "OK";
            case "cancel":
                return "キャンセル";
            case "yes":
                return "はい";
            case "no":
                return "いいえ";
            default:
                return role;
        }
    }

    /** 只有显式的 cancel 角色响应返回键（`yes_no` 不响应）。 */
    private static int cancelIndex(String[] roles) {
        for (int i = 0; i < roles.length; i++) {
            if ("cancel".equals(roles[i])) {
                return i + 1;
            }
        }
        return 0;
    }

    private static int iconRes(String icon) {
        switch (icon) {
            case "information":
                return android.R.drawable.ic_dialog_info;
            case "question":
                return android.R.drawable.ic_menu_help;
            case "warning":
            case "error":
                return android.R.drawable.ic_dialog_alert;
            default:
                return 0;
        }
    }

    // ------------------------------------------------------------ 数据

    /** 一条待呈现的 DIALOG（JSON 字段的只读副本）。 */
    private static final class Spec {
        final long id;
        final String caption;
        final String text;
        final String[] roles;
        final int defaultButton;
        final String icon;
        final boolean input;
        final int inputLimit;
        final String defaultInput;

        Spec(long id, String caption, String text, String[] roles, int defaultButton,
                String icon, boolean input, int inputLimit, String defaultInput) {
            this.id = id;
            this.caption = caption;
            this.text = text;
            this.roles = roles;
            this.defaultButton = defaultButton;
            this.icon = icon;
            this.input = input;
            this.inputLimit = inputLimit;
            this.defaultInput = defaultInput;
        }

        static Spec from(JSONObject obj) {
            long id = obj.optLong("id", -1);
            if (id < 0) {
                return null;
            }
            JSONArray buttons = obj.optJSONArray("buttons");
            List<String> roles = new ArrayList<>();
            if (buttons != null) {
                for (int i = 0; i < buttons.length(); i++) {
                    String role = buttons.optString(i, "");
                    if (!role.isEmpty()) {
                        roles.add(role);
                    }
                }
            }
            if (roles.isEmpty()) {
                roles.add("ok");
            }
            return new Spec(
                    id,
                    obj.optString("caption", ""),
                    obj.optString("text", ""),
                    roles.toArray(new String[0]),
                    obj.optInt("default_button", 0),
                    obj.optString("icon", "none"),
                    obj.optBoolean("input", false),
                    obj.optInt("input_limit", 0),
                    obj.optString("default_input", ""));
        }
    }
}
