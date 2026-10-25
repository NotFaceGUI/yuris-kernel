//! `gfx.*` 诊断码与构造助手。
//!
//! 所有错误/未实现都走 `yu-log::Diag`(唯一日志出口);`CompositorFrame`
//! 里只保留扁平化的 [`FrameDiag`],便于稳定序列化与 golden 比对。

use yu_log::{Boundary, Diag, Loc, Severity};

/// 稳定错误码表(`gfx.*` 命名空间;可计数、可进 golden、可直链 divergences)。
pub mod code {
    /// 图形域命令未实现/未取证。
    pub const COMMAND_UNIMPLEMENTED: &str = "gfx.command.unimplemented";
    /// 图形域命令名不在工程 ysc 表里。
    pub const COMMAND_UNKNOWN: &str = "gfx.command.unknown";
    /// 参数槽缺失(命令窗口不存在)。
    pub const ARG_MISSING: &str = "gfx.arg.missing";
    /// 参数类型不符(数值/字符串)。
    pub const ARG_TYPE: &str = "gfx.arg.type";
    /// 目标图层不存在(更新型命令)。
    pub const LAYER_MISSING: &str = "gfx.layer.missing";
    /// 图层 ID/路径非法。
    pub const LAYER_INVALID_ID: &str = "gfx.layer.invalid_id";
    /// 图层/图层组种类冲突(同一路径先建为组、后当图层用等)。
    pub const LAYER_KIND: &str = "gfx.layer.kind";
    /// 纹理/资源引用缺失或非法。
    pub const TEXTURE_MISSING: &str = "gfx.texture.missing";
    /// 混合模式未取证/无实现(4/8/10..18 的着色器路径)。
    pub const BLEND_UNSUPPORTED: &str = "gfx.blend.unsupported";
    /// 混合模式有状态表但语义未逐像素对拍(3/5/6/7/9)。
    pub const BLEND_UNVERIFIED: &str = "gfx.blend.unverified";
    /// 过渡语义缺项(TID 配对/双纹理取样等)。
    pub const TRANSITION_UNSUPPORTED: &str = "gfx.transition.unsupported";
    /// 外部掩码(`MID`)的取值通道未取证。
    pub const MASK_UNSUPPORTED: &str = "gfx.mask.unsupported";
    /// 滤镜/变换参数(`FX/FY/FRX/FCX/FEX/...`)写侧绑定未取证。
    pub const FILTER_UNSUPPORTED: &str = "gfx.filter.unsupported";
    /// 图层组/离屏语义缺项。
    pub const GROUP_UNSUPPORTED: &str = "gfx.group.unsupported";
    /// `CGACT[COPY/COPY2/SWAP...]` 的时序/区域语义缺项。
    pub const COPY_UNSUPPORTED: &str = "gfx.copy.unsupported";
    /// 多部件层几何/部件纹理语义未取证。
    pub const PARTS_UNSUPPORTED: &str = "gfx.parts.unsupported";
    /// `CGACT` 图像加工类操作(滤波/旋转/涂色/文字烘焙)。
    pub const IMAGEOP_UNSUPPORTED: &str = "gfx.imageop.unsupported";
    /// 文本模型缺项(字形栅格化/窗口定位/描边)。
    pub const TEXT_UNSUPPORTED: &str = "gfx.text.unsupported";
    /// 非视觉参数(如 `LINT`)已原样保留但未解释。
    pub const PARAM_NONVISUAL: &str = "gfx.param.nonvisual";
    /// 图像层显式尺寸:黑图可执行,资源图缩放语义未取证。
    pub const IMAGE_SIZE_UNSUPPORTED: &str = "gfx.image.size.unsupported";
    /// 窗口域命令/属性未实现或不属于 gfx。
    pub const WINDOW_UNSUPPORTED: &str = "gfx.window.unsupported";
    /// 诊断/操作日志达到硬上限,最旧项被丢弃。
    pub const LOG_TRUNCATED: &str = "gfx.log.truncated";
    /// INFO 多字段查询顺序未取证。
    pub const INFO_MULTI: &str = "gfx.info.multi";
    /// 查询目标图层/窗口不存在且字段不是 EXIST。
    pub const INFO_MISSING: &str = "gfx.info.missing";
    /// INFO 字段未取证/需要像素/鼠标/字体列表。
    pub const INFO_UNSUPPORTED: &str = "gfx.info.unsupported";
    /// 资源图尺寸需要资产元数据提供者(TODO DIV-GFX-024)。
    pub const INFO_ASSET_SIZE: &str = "gfx.info.asset_size";
    /// 内部不变量被破坏(应为不可能)。
    pub const INTERNAL: &str = "gfx.internal";
    /// 工程命令表缺失/不可用。
    pub const TABLE_MISSING: &str = "gfx.table.missing";
    /// `yscfg` 的扩展名探测序缺失(A5:缺表必须拒绝/显式挂号,不得兜底)。
    pub const CONFIG_PROBE_ORDER_MISSING: &str = "gfx.config.probe_order_missing";
    /// 主组 COPY 在首帧没有后缓冲源(DIV-GFX-051)。
    pub const COPY_NO_BACKBUFFER: &str = "gfx.copy.source.no_backbuffer";
    /// 复制源层已删除且没有命令时刻纹理快照(DIV-GFX-053)。
    pub const COPY_SOURCE_GONE: &str = "gfx.copy.source.gone";
}

pub fn fatal(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Fatal, Boundary::Domain).with_detail(detail)
}

pub fn recoverable(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Recoverable, Boundary::Domain).with_detail(detail)
}

pub fn unimplemented(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Unimplemented, Boundary::Domain).with_detail(detail)
}

pub fn info(code: &'static str, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Info, Boundary::Domain).with_detail(detail)
}

/// 给诊断补脚本位置(命令名/帧号)。图形域只拿得到事件帧号,
/// 因此 `where_` 用 `Loc::Script{file:"", label:None, pc:0, cmd}` 记录命令名。
pub fn at_command(mut diag: Diag, frame: u64, command: &str) -> Diag {
    diag.where_ = Loc::script(
        format!("gfx@frame{frame}"),
        None,
        0,
        Some(command.to_string()),
    );
    diag
}

/// 一条 FrameDiag 的发生位置(脚本坐标)。
///
/// gfx 是事件消费者,拿不到脚本文件名,只有 `DomainCall` 事件携带的
/// 脚本号与 pc(todo-silent-handling S1):显示形式 `s66 pc6028`。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiagAt {
    /// `s{script}` 形式的脚本标识。
    pub script: String,
    pub pc: u32,
    pub cmd: Option<String>,
}

impl DiagAt {
    pub fn display(&self) -> String {
        format!("{} pc{}", self.script, self.pc)
    }
}

/// 可稳定序列化的诊断快照(字段顺序固定;不含 `Loc` 的路径细节)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameDiag {
    pub code: String,
    pub severity: &'static str,
    pub command: String,
    pub detail: String,
    /// 同一帧内相同 `(code, command, detail)` 的出现次数(诊断去重但不丢频率)。
    pub count: u32,
    /// 发生位置(todo-silent-handling S1):同一 key 的不同脚本位置列表,
    /// 上限 [`MAX_FRAME_AT`];不为空时序列化进 frame JSON(空时不写键,
    /// 旧 golden 逐字节不变)。
    pub at: Vec<DiagAt>,
    /// 位置数超过上限时,被折叠的不同位置个数(显式计数,不静默丢)。
    pub at_more: u64,
}

/// 单条 FrameDiag 保留的不同位置上限;超出进 `at_more` 计数。
pub const MAX_FRAME_AT: usize = 8;

impl FrameDiag {
    pub fn new(
        code: impl Into<String>,
        severity: Severity,
        command: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.as_str(),
            command: command.to_string(),
            detail: detail.into(),
            count: 1,
            at: Vec::new(),
            at_more: 0,
        }
    }

    /// 登记一个发生位置(去重;超上限折进 `at_more`)。
    pub fn push_at(&mut self, at: &DiagAt) {
        if self.at.contains(at) {
            return;
        }
        if self.at.len() < MAX_FRAME_AT {
            self.at.push(at.clone());
        } else {
            self.at_more += 1;
        }
    }

    /// 位置段的一行显示(`s66 pc6028,s71 pc33+1`;空 = 无位置)。
    pub fn at_display(&self) -> String {
        let mut parts: Vec<String> = self.at.iter().map(|a| a.display()).collect();
        if self.at_more > 0 {
            parts.push(format!("+{}", self.at_more));
        }
        parts.join(",")
    }
}
