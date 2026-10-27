//! 图像加工缓存键(跨帧缓存用;与具体后端无关)。

/// 图像加工的源身份(资产 key / 1×1 黑 / 冻结快照)。
///
/// 用它而不是图层名做缓存键:同一份像素无论挂在哪个图层名下,加工结果相同
/// (EVID-T-10:图层名只是脚本约定,不参与引擎语义)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageSource {
    Asset(Vec<u8>),
    Solid,
    Snapshot(u64),
}

/// 图像加工的确定性签名(同一源 + 同一签名 ⇒ 同一结果)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageOpSig {
    /// `CGACT[SIZE]`(最近邻)/`SIZE2`(双线性)。
    Size {
        bilinear: bool,
        width: u32,
        height: u32,
    },
    /// `CGACT[REVERSE]`:RGB 取反。
    Reverse,
    /// `CGACT[HORIZONTAL]`:水平镜像。
    Horizontal,
    /// `CGACT[VERTICAL]`:垂直镜像。
    Vertical,
}

/// 跨帧图像结果缓存键。
///
/// 脚本每帧都会重发同样的 `CGACT[SIZE]`/`REVERSE`(title 期实测每帧 51 条
/// `SIZE` + 1 条 `REVERSE`);结果只依赖源像素与参数,缓存后帧内无像素工作
/// (EVID-J3-08)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageCacheKey {
    pub source: ImageSource,
    pub op: ImageOpSig,
    pub region: Option<(i64, i64, i64, i64)>,
}

/// 兼容旧名(任务内已有引用)。
pub type SizeSource = ImageSource;
/// 兼容旧名。
pub type SizeKey = ImageCacheKey;