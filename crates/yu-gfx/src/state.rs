//! `GfxState`:消费 `yu_rt::VmEvent`,维护图层树,产出 [`CompositorFrame`]。
//!
//! # 已实现的命令(ERIS 首屏实测覆盖)
//!
//! | 命令 | 首屏次数(frame 0) | 处理 |
//! |---|---:|---|
//! | `CG` | 30,668 | ID/IDNO/E/A/X/Y/Z/SX/SY/FILE/MODE + 原始参数留档 |
//! | `CGACT` | 280 | `COPY2`/`SWAP` 状态、`RECTPAINT*` 记为图像操作 |
//! | `CGEND` | 1,183 | 删层/删组(递归) |
//! | `FONT` | 3 | 字体属性快照 |
//! | `CGINFO`/`FONTINFO` | 606/1 | 只读查询,归 VM;gfx 只挂号 |
//!
//! 其余 `CG` 参数(`LINT/TSX/FX/...`)原样进入 [`Layer::extra`]/
//! [`crate::layer::FilterTransform`],并挂 `Unimplemented` 诊断;不静默丢。
//!
//! # 错误容错
//!
//! 引擎绘制函数失败会中止本帧(旧线 01 §5),但**状态更新失败**在脚本侧是
//! 可恢复的(手册 `cg/cg.html` 例 4 只声明“图层不存在 = 错误”)。本状态机
//! 对单条命令失败采用 `Recoverable` 诊断 + 跳过该命令,不终止整帧;依赖
//! 未取证语义的图层用 `LayerStatus::Unimplemented` 标记,由后端跳过。

use std::collections::{BTreeMap, BTreeSet};

use yu_log::Diag;
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::TableSet;
use yu_rt::value::Value;

use crate::error;
use crate::frame::{
    probe_order_from_codes, Clear, ColorSpace, CompositorFrame, TextureFilter, Viewport,
};
use crate::layer::{
    BlendConfidence, BlendMode, CopyId, EngineValue, FilterParam, ImageFormat, Layer, LayerId,
    LayerKind, LayerStatus, SizeU32, TextureRef, Transition,
};
use crate::op::{
    CopyKind, CopyOp, CopyRegion, CopySource, CopyTiming, ImageAction, ImageOp, MaskDraw, Op,
    OpStatus,
};
use crate::pass::{Pass, PassTarget};
use crate::path;
use crate::table::ParamTable;
use crate::text::{TextRun, TextStyle};
use crate::transition::TransitionDraw;

/// 已解析参数(名字/值都取自 `self.params`,先复制出来以释放不可变借用)。
struct OwnedArg {
    name: Option<String>,
    slot: u8,
    value: Value,
}

/// 图形域配置(视口/清屏/过滤/扩展名顺序/主窗口图层组)。
#[derive(Clone, Debug)]
pub struct GfxConfig {
    pub viewport: Viewport,
    pub clear: Clear,
    pub filter: TextureFilter,
    pub image_probe_order: Vec<ImageFormat>,
    /// 主窗口(`WINDOW[NO=0]`)显示的图层组。手册 `window/window.html`:
    /// 「NO=0 がメインウィンドウ、１～３がサブウィンドウ」「CGID=表示対象の
    /// レイヤセット」。默认 `:/W0`。
    pub main_layer_set: LayerId,
}

impl GfxConfig {
    pub fn new(viewport: Viewport) -> Self {
        Self {
            viewport,
            clear: Clear::default_black(),
            filter: TextureFilter::unknown(),
            image_probe_order: crate::frame::default_probe_order(),
            main_layer_set: LayerId::new(b":/W0".to_vec()),
        }
    }
}

struct Node {
    layer: Layer,
    parent: Option<LayerId>,
    children: BTreeSet<LayerId>,
}

/// 图形域状态机。
pub struct GfxState {
    params: ParamTable,
    config: GfxConfig,
    nodes: BTreeMap<LayerId, Node>,
    seq: u64,
    copy_seq: u64,
    frame: u64,
    style: TextStyle,
    text: Vec<TextRun>,
    copy_log: Vec<CopyOp>,
    image_log: Vec<ImageOp>,
    diags: Vec<error::FrameDiag>,
    seen_diags: BTreeSet<(String, String, String)>,
}

impl GfxState {
    /// 从工程表构造(命令表 = 参数名唯一来源)。
    pub fn from_tables(tables: &TableSet, mut config: GfxConfig) -> Result<Self, Diag> {
        let params = ParamTable::from_tables(tables);
        params.validate()?;
        let order = probe_order_from_codes(&tables.boot.config.image_ext_order);
        if !order.is_empty() {
            config.image_probe_order = order;
        }
        Ok(Self::new(params, config))
    }

    pub fn new(params: ParamTable, config: GfxConfig) -> Self {
        let mut s = Self {
            params,
            config,
            nodes: BTreeMap::new(),
            seq: 0,
            copy_seq: 0,
            frame: 0,
            style: TextStyle::default(),
            text: Vec::new(),
            copy_log: Vec::new(),
            image_log: Vec::new(),
            diags: Vec::new(),
            seen_diags: BTreeSet::new(),
        };
        s.ensure_w0();
        s
    }

    pub fn config(&self) -> &GfxConfig {
        &self.config
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.config.viewport = viewport;
        if let Some(w0) = self.nodes.get_mut(&Self::w0()) {
            w0.layer.kind = LayerKind::Group {
                size: Some(viewport_to_size(viewport)),
            };
        }
    }

    pub fn set_filter(&mut self, filter: TextureFilter) {
        self.config.filter = filter;
    }

    pub fn layer(&self, id: &LayerId) -> Option<&Layer> {
        self.nodes.get(id).map(|n| &n.layer)
    }

    pub fn diagnostics(&self) -> &[error::FrameDiag] {
        &self.diags
    }

    /// 纹理复制日志(状态生产;`frame()` 只输出仍被现存图层引用的项)。
    pub fn copies(&self) -> &[CopyOp] {
        &self.copy_log
    }

    /// 图像加工日志。
    pub fn images(&self) -> &[ImageOp] {
        &self.image_log
    }

    /// 消费一批事件。调用方应每帧 `drain_events` 后喂入,避免有界队列丢弃。
    pub fn consume_events(&mut self, events: &[VmEvent]) {
        for ev in events {
            self.on_event(ev);
        }
    }

    pub fn on_event(&mut self, ev: &VmEvent) {
        match ev {
            VmEvent::FrameStart { frame } => {
                self.frame = *frame;
                self.text.clear();
                // 复制/图像操作是**状态生产**(快照/纹理修改),不是逐帧易失;
                // 保留日志,frame() 只输出仍被现存图层引用的项。
            }
            VmEvent::FrameEnd { frame } => {
                self.frame = *frame;
            }
            VmEvent::Text { frame, id, bytes } => {
                self.frame = *frame;
                self.text.push(TextRun {
                    id: LayerId::new(id.as_bytes().to_vec()),
                    bytes: bytes.clone(),
                    style: self.style.clone(),
                    origin: None,
                    atlas: None,
                    frame: *frame,
                });
            }
            VmEvent::DomainCall {
                frame,
                domain: Domain::Graphics,
                name,
                args,
            } => {
                self.frame = *frame;
                self.dispatch(name, args);
            }
            _ => {}
        }
    }

    /// 构建当前帧的合成契约(只读;可重复调用,字节稳定)。
    pub fn frame(&self) -> CompositorFrame {
        let mut f = CompositorFrame::new(self.frame, self.config.viewport);
        f.clear = self.config.clear;
        f.filter = self.config.filter;
        f.image_probe_order = self.config.image_probe_order.clone();
        f.color_space = ColorSpace::GammaEncodedSrgb8;

        let mut passes = Vec::new();

        // 1. 纹理复制/图像加工(命令顺序)。
        //
        // 只输出**可执行**的项:目标仍引用该快照,且复制源在帧内仍存在。
        // 源已被 CGEND 删除的快照无法在渲染期重放,对应图层已在状态里标
        // `Unimplemented`;完整历史留在 `GfxState::copies()` 供审计。
        for c in &self.copy_log {
            let dest_live = self
                .nodes
                .get(&c.dest)
                .is_some_and(|n| matches!(n.layer.texture, TextureRef::Snapshot(id) if id == c.id));
            let source_live = match &c.source {
                CopySource::Layer(id) | CopySource::Group(id) => self.nodes.contains_key(id),
            };
            if !dest_live || !source_live {
                continue;
            }
            passes.push(Pass {
                target: PassTarget::Texture(c.dest.clone()),
                ops: vec![Op::Copy(c.clone())],
                clear: None,
            });
        }
        for i in &self.image_log {
            let source_live = self.nodes.contains_key(&i.source);
            let target_live = i.target.as_ref().is_none_or(|t| self.nodes.contains_key(t));
            if !source_live || !target_live {
                continue;
            }
            let target = i.target.clone().unwrap_or_else(|| i.source.clone());
            passes.push(Pass {
                target: PassTarget::Texture(target),
                ops: vec![Op::Image(i.clone())],
                clear: None,
            });
        }

        // 2. 组离屏 pass(后序;子组先于父组)。
        for child in self.sorted_children(None) {
            if self.nodes.get(&child).is_some_and(|n| n.layer.is_group()) {
                self.build_group_pass(&child, &mut passes);
            }
        }

        // 3. 屏幕 pass(最后)。
        //
        // 手册 window/window.html:NO=0 是主窗口,CGID 指定其显示的图层组;
        // 1..3 是子窗口。gfx 只把**主窗口图层组**(默认 :/W0)合成到屏幕;
        // 其余根组仍产出离屏 pass,交给后续 window/host 会话组合。
        let mut screen_ops = Vec::new();
        let main = self.config.main_layer_set.clone();
        if let Some(op) = self.screen_op(&main) {
            screen_ops.push(op);
        } else if !self.nodes.contains_key(&main) {
            // 合成配置没有主组(测试夹具/特殊宿主):退回所有顶层节点。
            for child in self.sorted_children(None) {
                if let Some(op) = self.screen_op(&child) {
                    screen_ops.push(op);
                }
            }
        }
        passes.push(Pass {
            target: PassTarget::Screen,
            ops: screen_ops,
            clear: Some(self.config.clear),
        });

        f.passes = passes;
        f.layers = self
            .nodes
            .values()
            .map(|n| n.layer.clone())
            .collect::<Vec<_>>();
        f.layers.sort_by_key(|a| (a.z, a.seq, a.id.clone()));
        f.text = self.text.clone();
        f.diagnostics = self.diags.clone();
        f.diagnostics.sort_by(|a, b| {
            (&a.code, &a.command, &a.detail).cmp(&(&b.code, &b.command, &b.detail))
        });
        f
    }

    // ------------------------------------------------------------ 命令分派

    fn dispatch(&mut self, name: &str, args: &[ArgValue]) {
        if !self.params.known(name) {
            self.push_diag(
                name,
                error::unimplemented(
                    error::code::COMMAND_UNKNOWN,
                    format!("命令 `{name}` 不在工程 ysc 表中,无法解析参数名"),
                ),
            );
            return;
        }
        match name {
            "CG" => self.apply_cg(args),
            "CGACT" => self.apply_cgact(args),
            "CGEND" => self.apply_cgend(args),
            "FONT" => self.apply_font(args),
            "CGINFO" => self.push_diag(
                name,
                error::unimplemented(
                    error::code::COMMAND_UNIMPLEMENTED,
                    "CGINFO 是只读查询(写 LET 变量),归 VM;gfx 不实现",
                ),
            ),
            "FONTINFO" => self.push_diag(
                name,
                error::unimplemented(
                    error::code::COMMAND_UNIMPLEMENTED,
                    "FONTINFO 是只读查询(写 LET 变量),归 VM;gfx 不实现",
                ),
            ),
            other => self.push_diag(
                other,
                error::unimplemented(
                    error::code::COMMAND_UNIMPLEMENTED,
                    "图形域命令未接线(首屏未使用)",
                ),
            ),
        }
    }

    // ------------------------------------------------------------ CG

    fn apply_cg(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("CG", args);
        let mut id: Option<Vec<u8>> = None;
        let mut idno: Option<i64> = None;
        let mut params: Vec<(String, EngineValue)> = Vec::new();
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("CG", r.slot);
                continue;
            };
            match name {
                "ID" => match &r.value {
                    Value::Str(b) => id = Some(b.clone()),
                    _ => self.arg_type("CG", "ID", "字符串", &r.value),
                },
                "IDNO" => match int_arg(&r.value) {
                    Some(n) => idno = Some(n),
                    None => self.arg_type("CG", "IDNO", "整数", &r.value),
                },
                _ => params.push((name.to_string(), EngineValue::from_value(&r.value))),
            }
        }
        let Some(id) = id else {
            self.push_diag(
                "CG",
                error::recoverable(error::code::ARG_MISSING, "CG 缺 ID(无法定位图层)"),
            );
            return;
        };
        let trailing = id.ends_with(b"/");
        let parsed = path::normalize(&id, idno, trailing);
        let is_dir = parsed.is_dir;

        let mut file_diags = Vec::new();
        let file = take_file(&params, &mut file_diags);
        for d in file_diags {
            self.push_diag("CG", d);
        }

        let existing = self.nodes.get(&parsed.path).map(|n| n.layer.is_group());
        if is_dir {
            if existing == Some(false) {
                self.push_diag(
                    "CG",
                    error::recoverable(
                        error::code::LAYER_KIND,
                        format!("`{}` 已是图像层,不能当图层组使用", parsed.path.display()),
                    ),
                );
                return;
            }
            if existing.is_none() {
                self.create_group(&parsed.path);
            }
            if file.is_some() {
                self.push_diag(
                    "CG",
                    error::unimplemented(
                        error::code::LAYER_KIND,
                        "FILE 只对图像层有效;图层组忽略 FILE",
                    ),
                );
            }
            let diags = {
                let node = self
                    .nodes
                    .get_mut(&parsed.path)
                    .expect("group just ensured");
                apply_layer_params(&mut node.layer, &params, true)
            };
            for d in diags {
                self.push_diag("CG", d);
            }
            return;
        }

        if existing == Some(true) {
            self.push_diag(
                "CG",
                error::recoverable(
                    error::code::LAYER_KIND,
                    format!("`{}` 已是图层组,不能当图像层使用", parsed.path.display()),
                ),
            );
            return;
        }

        if existing.is_none() {
            // 引擎处理体是“取/建节点”:即使没有 FILE 也建一个无纹理图层,
            // 绘制遍历按 `+0x90 == 0` 跳过。手册例 4 称“图层不存在 = 错误”,
            // 这里按引擎实际 get-or-create 处理(留档,不改变可见结果)。
            let texture = file.clone().unwrap_or(TextureRef::None);
            self.create_image(&parsed.path, texture);
            if file.is_none() {
                self.push_diag(
                    "CG",
                    error::info(
                        error::code::TEXTURE_MISSING,
                        "CG 未给 FILE:按引擎 get-or-create 建无纹理图层(遍历跳过)",
                    ),
                );
            }
        } else if let Some(texture) = file {
            // 手册:FILE 替换只换图,不重置 A/位置等属性。
            if let Some(node) = self.nodes.get_mut(&parsed.path) {
                node.layer.texture = texture;
            }
            // 旧快照/图像加工随旧纹理作废。
            self.copy_log.retain(|c| c.dest != parsed.path);
            self.image_log
                .retain(|i| i.source != parsed.path && i.target.as_ref() != Some(&parsed.path));
        }

        let diags = {
            let node = self
                .nodes
                .get_mut(&parsed.path)
                .expect("layer just ensured");
            apply_layer_params(&mut node.layer, &params, false)
        };
        for d in diags {
            self.push_diag("CG", d);
        }
    }

    // ------------------------------------------------------------ CGEND

    fn apply_cgend(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("CGEND", args);
        let mut id: Option<Vec<u8>> = None;
        let mut idno: Option<i64> = None;
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("CGEND", r.slot);
                continue;
            };
            match name {
                "ID" => match &r.value {
                    Value::Str(b) => id = Some(b.clone()),
                    _ => self.arg_type("CGEND", "ID", "字符串", &r.value),
                },
                "IDNO" => match int_arg(&r.value) {
                    Some(n) => idno = Some(n),
                    None => self.arg_type("CGEND", "IDNO", "整数", &r.value),
                },
                _ => {}
            }
        }
        let Some(id) = id else {
            self.push_diag(
                "CGEND",
                error::recoverable(error::code::ARG_MISSING, "CGEND 缺 ID"),
            );
            return;
        };
        let parsed = path::normalize(&id, idno, id.ends_with(b"/"));
        if path::is_root(&parsed.path) {
            self.push_diag(
                "CGEND",
                error::recoverable(error::code::LAYER_INVALID_ID, "不能删除根 `:/`"),
            );
            return;
        }
        if !self.nodes.contains_key(&parsed.path) {
            self.push_diag(
                "CGEND",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("删除不存在的图层 `{}`", parsed.path.display()),
                ),
            );
            return;
        }
        let removed = self.remove_subtree(&parsed.path);
        self.drop_ops_for(&removed);
        if parsed.path == Self::w0() && !self.nodes.contains_key(&Self::w0()) {
            // W0 是相对路径的默认基目录;删掉后立刻重建,保证后续相对 ID 可用。
            self.ensure_w0();
        }
    }

    // ------------------------------------------------------------ CGACT

    fn apply_cgact(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("CGACT", args);
        let mut ids: [Option<Vec<u8>>; 4] = [None, None, None, None];
        let mut idnos: [Option<i64>; 4] = [None, None, None, None];
        let mut params: Vec<(String, EngineValue)> = Vec::new();
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("CGACT", r.slot);
                continue;
            };
            let idx = match name {
                "ID" => Some(0),
                "ID2" => Some(1),
                "ID3" => Some(2),
                "ID4" => Some(3),
                _ => None,
            };
            if let Some(i) = idx {
                match &r.value {
                    Value::Str(b) => ids[i] = Some(b.clone()),
                    _ => self.arg_type("CGACT", name, "字符串", &r.value),
                }
                continue;
            }
            let idx = match name {
                "IDNO" => Some(0),
                "IDNO2" => Some(1),
                "IDNO3" => Some(2),
                "IDNO4" => Some(3),
                _ => None,
            };
            if let Some(i) = idx {
                match int_arg(&r.value) {
                    Some(n) => idnos[i] = Some(n),
                    None => self.arg_type("CGACT", name, "整数", &r.value),
                }
                continue;
            }
            params.push((name.to_string(), EngineValue::from_value(&r.value)));
        }

        let paths: Vec<Option<LayerId>> = (0..4)
            .map(|i| {
                ids[i]
                    .as_ref()
                    .map(|b| path::normalize(b, idnos[i], b.ends_with(b"/")).path)
            })
            .collect();
        let Some(source) = paths[0].clone() else {
            self.push_diag(
                "CGACT",
                error::recoverable(error::code::ARG_MISSING, "CGACT 缺 ID"),
            );
            return;
        };
        let dest = paths[1].clone().unwrap_or_else(|| source.clone());
        if paths[2].is_some() || paths[3].is_some() {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "ID3/ID4 的多图层运算语义未取证",
                ),
            );
        }
        let action_count = count_image_actions(&params);
        if action_count > 1 {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    format!("CGACT 同时给出 {action_count} 个图像动作;只记录第一个"),
                ),
            );
        }

        let set = find_value(&params, "SET");
        let set2 = find_value(&params, "SET2");
        let set_str = find_str(&params, "SETSTR");
        let x = find_int(&params, "X");
        let y = find_int(&params, "Y");
        let x2 = find_int(&params, "X2");
        let y2 = find_int(&params, "Y2");

        let copy_action = ["COPY", "COPY2", "TEXCOPY", "TEXSSCOPY"]
            .iter()
            .any(|n| flag(&params, n));
        let swap_action = flag(&params, "SWAP") || flag(&params, "SWAP2");
        if (copy_action || swap_action) && action_count > 0 {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "COPY/SWAP 与图像动作并存:引擎执行顺序未取证",
                ),
            );
        }

        for (name, kind) in [
            ("COPY", CopyKind::Copy),
            ("COPY2", CopyKind::Copy2),
            ("TEXCOPY", CopyKind::TexCopy),
            ("TEXSSCOPY", CopyKind::TexSsCopy),
        ] {
            if flag(&params, name) {
                self.cgact_copy(kind, &source, &dest, x, y, x2, y2);
                return;
            }
        }
        if flag(&params, "SWAP") {
            self.cgact_swap(&source, &dest, false);
            return;
        }
        if flag(&params, "SWAP2") {
            self.cgact_swap(&source, &dest, true);
            return;
        }

        if let Some((name, action)) = first_image_action(&params) {
            self.cgact_image(
                name,
                action,
                &source,
                paths[1].clone(),
                set,
                set2,
                set_str,
                x,
                y,
                x2,
                y2,
            );
            return;
        }

        // 没有任何动作参数:CGACT 的属性修改(例如只给 SET)语义未取证。
        if !params.is_empty() {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "CGACT 无动作参数(仅属性/坐标)语义未取证",
                ),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cgact_copy(
        &mut self,
        kind: CopyKind,
        source: &LayerId,
        dest: &LayerId,
        x: Option<i64>,
        y: Option<i64>,
        x2: Option<i64>,
        y2: Option<i64>,
    ) {
        if !self.nodes.contains_key(source) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("复制源 `{}` 不存在", source.display()),
                ),
            );
            return;
        }
        let source_is_group = self.nodes[source].layer.is_group();
        if !self.nodes.contains_key(dest) {
            // 手册 cg/cgact.html 例 30:目标层可作为复制结果出现。
            self.create_image(dest, TextureRef::None);
        }
        if self.nodes.get(dest).is_some_and(|n| n.layer.is_group()) {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::COPY_UNSUPPORTED,
                    "复制目标为图层组:复制到组合成纹理的语义未取证",
                ),
            );
            return;
        }
        let copy_id = CopyId(self.next_copy());
        let region = if x.is_some() || y.is_some() || x2.is_some() || y2.is_some() {
            CopyRegion::Partial {
                x: x.unwrap_or(0),
                y: y.unwrap_or(0),
                x2: x2.unwrap_or(0),
                y2: y2.unwrap_or(0),
            }
        } else {
            CopyRegion::Full
        };
        self.copy_log.retain(|c| c.dest != *dest);
        self.image_log
            .retain(|i| i.target.as_ref().is_none_or(|t| t != dest));
        self.copy_log.push(CopyOp {
            id: copy_id,
            kind,
            source: if source_is_group {
                CopySource::Group(source.clone())
            } else {
                CopySource::Layer(source.clone())
            },
            dest: dest.clone(),
            region,
            timing: CopyTiming::LastRenderedFrame,
            status: OpStatus::Unimplemented("gfx.copy.unsupported"),
        });
        if kind == CopyKind::Copy {
            let attrs = self.nodes[source].layer.clone();
            if let Some(node) = self.nodes.get_mut(dest) {
                copy_attributes(&attrs, &mut node.layer);
            }
        }
        if let Some(node) = self.nodes.get_mut(dest) {
            node.layer.texture = TextureRef::Snapshot(copy_id);
            mark_unimplemented(&mut node.layer, "gfx.copy.unsupported");
        }
        self.push_diag(
            "CGACT",
            error::unimplemented(
                error::code::COPY_UNSUPPORTED,
                format!(
                    "{} 的时序=上一帧已渲染目标;区域/目标纹理语义未逐项取证",
                    kind.name()
                ),
            ),
        );
    }

    fn cgact_swap(&mut self, source: &LayerId, dest: &LayerId, image_only: bool) {
        if !self.nodes.contains_key(source) || !self.nodes.contains_key(dest) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!(
                        "SWAP 源/目标不存在: `{}` / `{}`",
                        source.display(),
                        dest.display()
                    ),
                ),
            );
            return;
        }
        if self.nodes[source].layer.is_group() || self.nodes[dest].layer.is_group() {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::COPY_UNSUPPORTED,
                    "图层组 SWAP 语义未取证(仅图像层)",
                ),
            );
            return;
        }
        // 引擎 FUN_0042f57d:
        // - 总是交换纹理/图像名与 +0x68/+0x70(X/Y);
        // - 模式 2 额外交换 +0x89(E)/+0x80(A)/+0x82(T)/+0x84(TA)/+0x87(MODE)。
        // 手册:SWAP=图像+属性,SWAP2=仅图像。模式 1/2 与 SWAP/SWAP2 的
        // 精确对应为 Likely;这里按“SWAP2 只换图+坐标、SWAP 再换可见属性”实现。
        let src = self.nodes[source].layer.clone();
        let dst = self.nodes[dest].layer.clone();
        {
            let s = self.nodes.get_mut(source).expect("checked");
            s.layer.texture = dst.texture.clone();
            s.layer.transform.x = dst.transform.x;
            s.layer.transform.y = dst.transform.y;
        }
        {
            let d = self.nodes.get_mut(dest).expect("checked");
            d.layer.texture = src.texture.clone();
            d.layer.transform.x = src.transform.x;
            d.layer.transform.y = src.transform.y;
        }
        if !image_only {
            let swap_attrs = |a: &mut Layer, b: &Layer| {
                a.visible = b.visible;
                a.alpha = b.alpha;
                a.blend = b.blend;
                a.transition = b.transition.clone();
            };
            let s = self.nodes.get_mut(source).expect("checked");
            swap_attrs(&mut s.layer, &dst);
            let d = self.nodes.get_mut(dest).expect("checked");
            swap_attrs(&mut d.layer, &src);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cgact_image(
        &mut self,
        name: &str,
        action: ImageAction,
        source: &LayerId,
        target: Option<LayerId>,
        set: Option<EngineValue>,
        set2: Option<EngineValue>,
        set_str: Option<Vec<u8>>,
        x: Option<i64>,
        y: Option<i64>,
        x2: Option<i64>,
        y2: Option<i64>,
    ) {
        if !self.nodes.contains_key(source) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("图像加工源 `{}` 不存在", source.display()),
                ),
            );
            return;
        }
        let target = target.or_else(|| Some(source.clone()));
        if let Some(t) = &target {
            if !self.nodes.contains_key(t) {
                self.create_image(t, TextureRef::None);
            }
            if self.nodes.get(t).is_some_and(|n| n.layer.is_group()) {
                self.push_diag(
                    "CGACT",
                    error::unimplemented(
                        error::code::IMAGEOP_UNSUPPORTED,
                        "图像加工目标为图层组:写入组合成纹理的语义未取证",
                    ),
                );
                return;
            }
        }
        self.image_log.push(ImageOp {
            source: source.clone(),
            target: target.clone(),
            action,
            set,
            set2,
            set_str,
            x,
            y,
            x2,
            y2,
            status: OpStatus::Unimplemented("gfx.imageop.unsupported"),
        });
        if let Some(node) = self.nodes.get_mut(source) {
            mark_unimplemented(&mut node.layer, "gfx.imageop.unsupported");
        }
        if let Some(t) = target {
            if let Some(node) = self.nodes.get_mut(&t) {
                mark_unimplemented(&mut node.layer, "gfx.imageop.unsupported");
            }
        }
        self.push_diag(
            "CGACT",
            error::unimplemented(
                error::code::IMAGEOP_UNSUPPORTED,
                format!("图像加工 `{name}` 的像素算法未取证;只记录原始参数"),
            ),
        );
    }

    // ------------------------------------------------------------ FONT / TEXT

    fn apply_font(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("FONT", args);
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("FONT", r.slot);
                continue;
            };
            match name {
                "NAME" => match &r.value {
                    Value::Str(b) => self.style.name = Some(b.clone()),
                    _ => self.arg_type("FONT", name, "字符串", &r.value),
                },
                "COLOR" => match int_arg(&r.value) {
                    Some(n) => self.style.color = n as u32,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "COLOR2" => match int_arg(&r.value) {
                    Some(n) => self.style.color2 = Some(n as u32),
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "SX" => match int_arg(&r.value) {
                    Some(n) => self.style.size_x = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "SY" => match int_arg(&r.value) {
                    Some(n) => self.style.size_y = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "BOLD" => self.style.bold = flag_value(&r.value),
                "ANTIALIASING" => self.style.antialias = flag_value(&r.value),
                "DRAWMODE" => match int_arg(&r.value) {
                    Some(n) => self.style.draw_mode = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "NO" | "BNO" => self.push_diag(
                    "FONT",
                    error::unimplemented(
                        error::code::TEXT_UNSUPPORTED,
                        format!("FONT[{name}] 字体列表序号未取证;保留当前字体"),
                    ),
                ),
                _ => self.push_diag(
                    "FONT",
                    error::unimplemented(
                        error::code::PARAM_NONVISUAL,
                        format!("FONT 参数 `{name}` 未分类"),
                    ),
                ),
            }
        }
    }

    // ------------------------------------------------------------ 内部

    fn w0() -> LayerId {
        LayerId::new(b":/W0".to_vec())
    }

    fn resolved_owned(&self, command: &str, args: &[ArgValue]) -> Vec<OwnedArg> {
        self.params
            .resolve(command, args)
            .into_iter()
            .map(|r| OwnedArg {
                name: r.name.map(str::to_string),
                slot: r.slot,
                value: r.value.clone(),
            })
            .collect()
    }

    fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    fn next_copy(&mut self) -> u64 {
        let c = self.copy_seq;
        self.copy_seq += 1;
        c
    }

    fn ensure_w0(&mut self) {
        let w0 = Self::w0();
        if self.nodes.contains_key(&w0) {
            return;
        }
        let layer = Layer {
            id: w0.clone(),
            seq: self.next_seq(),
            z: 0,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Group {
                size: Some(viewport_to_size(self.config.viewport)),
            },
            texture: TextureRef::None,
            parts: Vec::new(),
            transform: Default::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            extra: BTreeMap::new(),
        };
        self.nodes.insert(
            w0,
            Node {
                layer,
                parent: None,
                children: BTreeSet::new(),
            },
        );
    }

    fn create_group(&mut self, path: &LayerId) {
        if self.nodes.contains_key(path) {
            return;
        }
        self.ensure_parents(path);
        let layer = Layer {
            id: path.clone(),
            seq: self.next_seq(),
            z: 0,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Group { size: None },
            texture: TextureRef::None,
            parts: Vec::new(),
            transform: Default::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            extra: BTreeMap::new(),
        };
        self.insert_node(path, layer);
    }

    fn create_image(&mut self, path: &LayerId, texture: TextureRef) {
        if self.nodes.contains_key(path) {
            return;
        }
        self.ensure_parents(path);
        let layer = Layer {
            id: path.clone(),
            seq: self.next_seq(),
            z: 0,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Image,
            texture,
            parts: Vec::new(),
            transform: Default::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            extra: BTreeMap::new(),
        };
        self.insert_node(path, layer);
    }

    fn ensure_parents(&mut self, path: &LayerId) {
        let Some(parent) = path::parent(path) else {
            return;
        };
        if path::is_root(&parent) {
            return;
        }
        if !self.nodes.contains_key(&parent) {
            self.ensure_parents(&parent);
            self.create_group(&parent);
        }
    }

    fn insert_node(&mut self, path: &LayerId, layer: Layer) {
        let parent = path::parent(path).filter(|p| !path::is_root(p));
        self.nodes.insert(
            path.clone(),
            Node {
                layer,
                parent: parent.clone(),
                children: BTreeSet::new(),
            },
        );
        if let Some(parent) = parent {
            if let Some(p) = self.nodes.get_mut(&parent) {
                p.children.insert(path.clone());
            }
        }
    }

    fn remove_subtree(&mut self, path: &LayerId) -> BTreeSet<LayerId> {
        let mut removed = BTreeSet::new();
        let mut stack = vec![path.clone()];
        while let Some(p) = stack.pop() {
            if let Some(node) = self.nodes.remove(&p) {
                removed.insert(p.clone());
                stack.extend(node.children);
            }
        }
        if let Some(parent) = path::parent(path).filter(|p| !path::is_root(p)) {
            if let Some(p) = self.nodes.get_mut(&parent) {
                p.children.remove(path);
            }
        }
        removed
    }

    fn drop_ops_for(&mut self, removed: &BTreeSet<LayerId>) {
        // 快照是独立纹理:源图层被删后,已完成的复制仍需留给后端物化。
        self.copy_log.retain(|c| !removed.contains(&c.dest));
        // 图像加工同理:只要目标还在,操作历史仍描述目标纹理。
        self.image_log
            .retain(|i| i.target.as_ref().is_none_or(|t| !removed.contains(t)));
    }

    fn sorted_children(&self, parent: Option<&LayerId>) -> Vec<LayerId> {
        let mut ids: Vec<LayerId> = if let Some(parent) = parent {
            self.nodes
                .get(parent)
                .map(|n| n.children.iter().cloned().collect())
                .unwrap_or_default()
        } else {
            self.nodes
                .values()
                .filter(|n| n.parent.is_none())
                .map(|n| n.layer.id.clone())
                .collect()
        };
        ids.sort_by(|a, b| {
            let la = &self.nodes[a].layer;
            let lb = &self.nodes[b].layer;
            (la.z, la.seq, a.clone()).cmp(&(lb.z, lb.seq, b.clone()))
        });
        ids
    }

    fn group_has_visible_content(&self, id: &LayerId) -> bool {
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        node.children.iter().any(|c| {
            self.nodes.get(c).is_some_and(|n| {
                if !n.layer.visible {
                    return false;
                }
                if n.layer.is_group() {
                    self.group_has_visible_content(c)
                } else {
                    !matches!(n.layer.texture, TextureRef::None)
                }
            })
        })
    }

    /// 生成组的离屏 pass(后序)。返回是否生成了非空 pass。
    fn build_group_pass(&self, id: &LayerId, out: &mut Vec<Pass>) -> bool {
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        let children = self.sorted_children(Some(id));
        let mut ops = Vec::new();
        for child in children {
            let Some(cnode) = self.nodes.get(&child) else {
                continue;
            };
            if cnode.layer.is_group() {
                let emitted = self.build_group_pass(&child, out);
                if emitted && cnode.layer.visible {
                    ops.push(Op::Group(crate::group::GroupDraw {
                        layer: cnode.layer.clone(),
                        offscreen: child,
                    }));
                }
            } else if cnode.layer.visible && !matches!(cnode.layer.texture, TextureRef::None) {
                ops.push(self.op_for_layer(&cnode.layer));
            }
        }
        if ops.is_empty() {
            return false;
        }
        let _ = node;
        out.push(Pass {
            target: PassTarget::Offscreen(id.clone()),
            ops,
            // 组离屏背景 = 透明黑;组自身 A/MODE 作用于合成结果。
            clear: Some(Clear::explicit([0, 0, 0, 0])),
        });
        true
    }

    /// 一个顶层节点在屏幕 pass 里的 op(不可见/空组返回 `None`)。
    fn screen_op(&self, id: &LayerId) -> Option<Op> {
        let node = self.nodes.get(id)?;
        if !node.layer.visible {
            return None;
        }
        if node.layer.is_group() {
            if self.group_has_visible_content(id) {
                Some(Op::Group(crate::group::GroupDraw {
                    layer: node.layer.clone(),
                    offscreen: id.clone(),
                }))
            } else {
                None
            }
        } else if matches!(node.layer.texture, TextureRef::None) {
            None
        } else {
            Some(self.op_for_layer(&node.layer))
        }
    }

    fn op_for_layer(&self, layer: &Layer) -> Op {
        if !layer.parts.is_empty() {
            let mut l = layer.clone();
            mark_unimplemented(&mut l, "gfx.parts.unsupported");
            return Op::Layer(l);
        }
        if let Some(t) = &layer.transition {
            if let Some(fade) = t.fade_value() {
                let to = t.tid.as_ref().and_then(|tid| {
                    let n = self.nodes.get(tid)?;
                    Some(if n.layer.is_group() {
                        TextureRef::Offscreen(tid.clone())
                    } else {
                        n.layer.texture.clone()
                    })
                });
                let mut l = layer.clone();
                if to.is_none() {
                    mark_unimplemented(&mut l, "gfx.transition.unsupported");
                }
                return Op::Transition(TransitionDraw {
                    from: l.texture.clone(),
                    to,
                    fade,
                    tid: t.tid.clone(),
                    layer: l,
                });
            }
        }
        if let Some(mask) = &layer.mask {
            return Op::Mask(MaskDraw {
                layer: layer.clone(),
                mask: mask.clone(),
            });
        }
        Op::Layer(layer.clone())
    }

    fn push_diag(&mut self, command: &str, d: Diag) {
        let key = (d.code.to_string(), command.to_string(), d.detail.clone());
        if self.seen_diags.insert(key) {
            self.diags.push(error::FrameDiag::new(
                d.code.to_string(),
                d.severity,
                command,
                d.detail,
            ));
        }
    }

    fn arg_slot_missing(&mut self, command: &str, slot: u8) {
        self.push_diag(
            command,
            error::unimplemented(
                error::code::ARG_MISSING,
                format!("槽 {slot} 不在 `{command}` 的 ysc 参数表内"),
            ),
        );
    }

    fn arg_type(&mut self, command: &str, name: &str, expected: &str, value: &Value) {
        self.push_diag(
            command,
            error::recoverable(
                error::code::ARG_TYPE,
                format!("参数 `{name}` 期望 {expected},实际 {}", value.type_name()),
            ),
        );
    }
}

// ---------------------------------------------------------------- 参数应用

fn viewport_to_size(v: Viewport) -> SizeU32 {
    SizeU32 {
        width: v.width,
        height: v.height,
    }
}

fn int_arg(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 && f.is_finite() => Some(*f as i64),
        _ => None,
    }
}

fn flag_value(v: &Value) -> bool {
    match v {
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(b) => !b.is_empty(),
    }
}

fn find_value(params: &[(String, EngineValue)], name: &str) -> Option<EngineValue> {
    params
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
}

fn find_int(params: &[(String, EngineValue)], name: &str) -> Option<i64> {
    find_value(params, name).and_then(|v| v.as_int())
}

fn find_str(params: &[(String, EngineValue)], name: &str) -> Option<Vec<u8>> {
    find_value(params, name).and_then(|v| v.as_bytes().map(|b| b.to_vec()))
}

fn flag(params: &[(String, EngineValue)], name: &str) -> bool {
    find_value(params, name).is_some_and(|v| match v {
        EngineValue::Int(i) => i != 0,
        EngineValue::Float(f) => f != 0.0,
        EngineValue::Str(b) => !b.is_empty(),
    })
}

fn take_file(params: &[(String, EngineValue)], diags: &mut Vec<Diag>) -> Option<TextureRef> {
    for (name, value) in params {
        if name != "FILE" {
            continue;
        }
        return match value {
            EngineValue::Str(b) if b.is_empty() => Some(TextureRef::SolidBlack1x1),
            EngineValue::Str(b) => Some(TextureRef::Asset(b.clone())),
            _ => {
                diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `FILE` 期望字符串",
                ));
                None
            }
        };
    }
    None
}

fn mark_unimplemented(layer: &mut Layer, reason: &'static str) {
    if matches!(layer.status, LayerStatus::Executable) {
        layer.status = LayerStatus::Unimplemented(reason);
    }
}

fn copy_attributes(src: &Layer, dst: &mut Layer) {
    dst.alpha = src.alpha;
    dst.blend = src.blend;
    dst.visible = src.visible;
    dst.z = src.z;
    dst.transform = src.transform.clone();
    dst.parts = src.parts.clone();
    dst.mask = src.mask.clone();
    dst.transition = src.transition.clone();
    dst.clip = src.clip;
    dst.extra = src.extra.clone();
    dst.status = src.status;
}

static IMAGE_ACTIONS: &[(&str, ImageAction)] = &[
    ("RECTPAINT", ImageAction::RectPaint),
    ("RECTPAINT2", ImageAction::RectPaint2),
    ("RECTPAINT3", ImageAction::RectPaint3),
    ("TEXT", ImageAction::Text),
    ("SIZE", ImageAction::SizeNearest),
    ("SIZE2", ImageAction::SizeBilinear),
    ("ROTATION", ImageAction::Rotation),
    ("MERGE", ImageAction::Merge),
    ("MERGE2", ImageAction::Merge2),
    ("SIZE3", ImageAction::Other("size3")),
    ("SIZE4", ImageAction::Other("size4")),
    ("SIZE5", ImageAction::Other("size5")),
    ("SAIDO", ImageAction::Other("saido")),
    ("MEIDO", ImageAction::Other("meido")),
    ("SEPIA", ImageAction::Other("sepia")),
    ("AKARUSA", ImageAction::Other("akarusa")),
    ("MONO", ImageAction::Other("mono")),
    ("VERTICAL", ImageAction::Other("vertical")),
    ("HORIZONTAL", ImageAction::Other("horizontal")),
    ("MOSAIC", ImageAction::Other("mosaic")),
    ("BLUR", ImageAction::Other("blur")),
    ("DIFFUSION", ImageAction::Other("diffusion")),
    ("REVERSE", ImageAction::Other("reverse")),
    ("WHIRLPOOL", ImageAction::Other("whirlpool")),
    ("RASTER", ImageAction::Other("raster")),
    ("RASTER2", ImageAction::Other("raster2")),
    ("RASTER3", ImageAction::Other("raster3")),
    ("TRANSITION", ImageAction::Other("transition")),
];

fn count_image_actions(params: &[(String, EngineValue)]) -> usize {
    IMAGE_ACTIONS
        .iter()
        .filter(|(candidate, _)| flag(params, candidate))
        .count()
}

fn first_image_action(params: &[(String, EngineValue)]) -> Option<(&'static str, ImageAction)> {
    // 参数顺序即编译器窗口顺序;取第一个出现的动作。
    for (name, _) in params {
        for (candidate, action) in IMAGE_ACTIONS {
            if name == candidate && flag(params, candidate) {
                return Some((candidate, action.clone()));
            }
        }
    }
    None
}

/// 应用 `CG` 属性参数。返回诊断(不直接写 self,避免借用冲突)。
fn apply_layer_params(
    layer: &mut Layer,
    params: &[(String, EngineValue)],
    is_group: bool,
) -> Vec<Diag> {
    let mut diags = Vec::new();
    let mut sx: Option<i64> = None;
    let mut sy: Option<i64> = None;
    for (name, value) in params {
        match name.as_str() {
            "FILE" | "ID" | "IDNO" => {}
            "E" => match value.as_int() {
                Some(n) => layer.visible = n != 0,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `E` 期望整数",
                )),
            },
            "A" => match value.as_int() {
                Some(n) => match i16::try_from(n) {
                    Ok(v) => layer.alpha = v,
                    Err(_) => diags.push(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("参数 `A={n}` 超出 s16 存储范围"),
                    )),
                },
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `A` 期望整数",
                )),
            },
            "X" => match value.as_int() {
                Some(n) => layer.transform.x = n,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `X` 期望整数",
                )),
            },
            "Y" => match value.as_int() {
                Some(n) => layer.transform.y = n,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `Y` 期望整数",
                )),
            },
            "Z" => match value.as_int() {
                Some(n) => layer.z = n,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `Z` 期望整数",
                )),
            },
            "SX" => sx = value.as_int(),
            "SY" => sy = value.as_int(),
            "MODE" => match value.as_int() {
                Some(n) => match BlendMode::from_code(n) {
                    Some(m) => {
                        layer.blend = m;
                        match m.confidence() {
                            BlendConfidence::Confirmed | BlendConfidence::Likely => {}
                            BlendConfidence::Unverified => {
                                mark_unimplemented(layer, "gfx.blend.unverified");
                                diags.push(error::unimplemented(
                                    error::code::BLEND_UNVERIFIED,
                                    format!("MODE={n}({}) 固定管线状态已取证,语义未对拍", m.name()),
                                ));
                            }
                            BlendConfidence::Unimplemented => {
                                mark_unimplemented(layer, "gfx.blend.unsupported");
                                diags.push(error::unimplemented(
                                    error::code::BLEND_UNSUPPORTED,
                                    format!("MODE={n}({}) 着色器路径未取证", m.name()),
                                ));
                            }
                        }
                    }
                    None => {
                        layer.extra.insert("MODE".to_string(), EngineValue::Int(n));
                        mark_unimplemented(layer, "gfx.blend.unsupported");
                        diags.push(error::unimplemented(
                            error::code::BLEND_UNSUPPORTED,
                            format!("MODE={n} 超出手册 0..18"),
                        ));
                    }
                },
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `MODE` 期望整数",
                )),
            },
            "T" | "TA" => {
                let Some(n) = value.as_int() else {
                    diags.push(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("参数 `{name}` 期望整数"),
                    ));
                    continue;
                };
                let Ok(v) = i16::try_from(n) else {
                    diags.push(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("参数 `{name}={n}` 超出 s16 存储范围"),
                    ));
                    continue;
                };
                let t = layer.transition.get_or_insert_with(Transition::default);
                if name == "T" {
                    t.t = v;
                } else {
                    t.ta = v;
                }
            }
            "TID" => match value.as_bytes() {
                Some(b) => {
                    let p = path::normalize(b, None, false).path;
                    layer.transition.get_or_insert_with(Transition::default).tid = Some(p);
                }
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `TID` 期望字符串",
                )),
            },
            "MID" => match value.as_bytes() {
                Some(b) => {
                    layer.mask = Some(path::normalize(b, None, false).path);
                    mark_unimplemented(layer, "gfx.mask.unsupported");
                    diags.push(error::unimplemented(
                        error::code::MASK_UNSUPPORTED,
                        "外部掩码 MID 的取值通道(α/亮度)未取证",
                    ));
                }
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `MID` 期望字符串",
                )),
            },
            other => {
                if let Some(fp) = FilterParam::from_name(other) {
                    layer.transform.filter.set(fp, value.clone());
                    mark_unimplemented(layer, "gfx.filter.unsupported");
                    diags.push(error::unimplemented(
                        error::code::FILTER_UNSUPPORTED,
                        format!("滤镜/仿射参数 `{other}` 的槽位→偏移绑定未取证"),
                    ));
                } else {
                    layer.extra.insert(other.to_string(), value.clone());
                    diags.push(error::unimplemented(
                        error::code::PARAM_NONVISUAL,
                        format!("参数 `{other}` 已原样保留,视觉语义未分类"),
                    ));
                }
            }
        }
    }

    if is_group {
        let current = layer.group_size();
        let sx = sx.or(current.map(|s| i64::from(s.width)));
        let sy = sy.or(current.map(|s| i64::from(s.height)));
        if let (Some(sx), Some(sy)) = (sx, sy) {
            match (u32::try_from(sx), u32::try_from(sy)) {
                (Ok(w), Ok(h)) => {
                    layer.kind = LayerKind::Group {
                        size: Some(SizeU32 {
                            width: w,
                            height: h,
                        }),
                    };
                }
                _ => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    format!("图层组 SX={sx} SY={sy} 超出 u32 范围"),
                )),
            }
        }
    } else if sx.is_some() || sy.is_some() {
        // 手册:CG SX/SY 只对图层组有效。图像层的尺寸来自纹理;
        // 这里只留档,不改变合成几何。
        if let Some(v) = sx {
            layer.extra.insert("SX".to_string(), EngineValue::Int(v));
        }
        if let Some(v) = sy {
            layer.extra.insert("SY".to_string(), EngineValue::Int(v));
        }
        diags.push(error::info(
            error::code::PARAM_NONVISUAL,
            "CG SX/SY 对图像层只留档(手册:仅图层组有效)",
        ));
    }
    diags
}
