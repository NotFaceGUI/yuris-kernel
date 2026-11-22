//! 媒体与只读信息:MOVIE/YMV/MOVIEINFO/MATH3D/INFO/SOUNDINFO。
//!
//! 由 `handlers/mod.rs` 的命令族拆分而来;只搬运,函数体逐字不变。

use super::helpers::*;
use super::*;

impl Vm {

    // ------------------------------------------------------------ MOVIE(全屏)

    /// 取已求值参数里的整数(`name` 按工程 ysc 表匹配,不写死槽号)。
    pub(super) fn movie_int_arg(&self, params: &[String], args: &[ArgValue], name: &str) -> Option<i64> {
        let slot = params.iter().position(|p| p == name)?;
        args.iter()
            .find(|a| usize::from(a.slot) == slot)
            .and_then(|a| match &a.value {
                Value::Int(v) => Some(*v),
                Value::Float(v) => Some(v.round() as i64),
                _ => None,
            })
    }

    /// 取已求值参数里的浮点(`X/Y/SX/SY` 是 F 槽,整数原值也接受)。
    pub(super) fn movie_float_arg(&self, params: &[String], args: &[ArgValue], name: &str) -> Option<f64> {
        let slot = params.iter().position(|p| p == name)?;
        args.iter()
            .find(|a| usize::from(a.slot) == slot)
            .and_then(|a| match &a.value {
                Value::Int(v) => Some(*v as f64),
                Value::Float(v) => Some(*v),
                _ => None,
            })
    }

    /// 探测影片容器的编解码(官方 `MOVIEINFO[CODEC]` 的同一判定)。
    pub(super) fn probe_movie_codec(&mut self, file: &[u8]) -> yu_format::movie::MovieCodec {
        // 只读文件头(`PROBE_BYTES` 字节):CODEC 判定不需要整部影片。
        // 旧实现走 `open_asset` 整读,预检大影片时会白占一份内存。
        match self.open_asset_prefix(file, yu_format::movie::PROBE_BYTES) {
            Some(bytes) => {
                let head = &bytes[..bytes.len().min(yu_format::movie::PROBE_BYTES)];
                yu_format::movie::probe_codec(head)
            }
            None => yu_format::movie::MovieCodec::Missing,
        }
    }

    /// `MOVIEINFO[PLAY, X, Y, SX, SY, VOLUME, CODEC, LET]`(REA `0x44ba2c`)。
    ///
    /// 官方七字段是**顺序 `if` + 各自写 `LET` + `ret`**(`0x44ba60..0x44bbde`):
    /// 多字段同时给出时**槽号最小的字段生效**;一个字段都没给时直接返回,
    /// 不写 `LET`。判定看的是参数**存在标志**(`0x684160..0x684166` 逐字节
    /// `test/jne`),不是参数值是否非零——`PLAY=0` 同样应写回 0。
    ///
    /// `CODEC` 分支(`0x44bac9..0x44bcfc`)把 `CODEC` **参数本身**当作待探测
    /// 文件名:官方先拷 CWD/工程基路径 `0x681300`,再把该参数字符串追加到
    /// 尾部后 `fopen`;文件不存在写 0,MPEG-1 写 1,MPEG-2 写 2,其它写 255。
    /// 因此它**不能**读 `MOVIE[FILE]` 的当前文件名——ERIS `es_MoviePlay.yst`
    /// 正是在 `MOVIE[FILE]` 之前用 `MOVIEINFO[CODEC=$FN]` 做存在性/编码
    /// 预检,旧实现读当前文件名(此时为空)恒得 0,宏路径随即
    /// `IF[@codeccheck==0] IFBREAK[LV=3]` 放弃播放。内核的 VFS 根就是官方
    /// 的基路径,直接探测参数即可。
    pub(super) fn op_movieinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let params: &[String] = &cmd.param_names;
        let mut targets: Vec<crate::expr::LValue> = Vec::new();
        let mut event_args: Vec<ArgValue> = Vec::new();
        let mut flags: smallvec::SmallVec<[&str; 8]> = smallvec::SmallVec::new();
        // `CODEC` 的参数值 = 待探测文件名(不是当前影片)。
        let mut codec_file: Option<Vec<u8>> = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            if pname == "LET" {
                let lv = self.eval_lvalue_window(st, w)?;
                let var = self.canonical_var(lv.var);
                if let Ok(v) = self.read_var(st, var, &lv.indices) {
                    event_args.push(ArgValue::with_ref(w.rec.kind, v, var));
                }
                targets.push(lv);
                continue;
            }
            // 官方按「参数存在」选择字段(`0x684160..0x684166` 的存在标志),
            // 不看数值是否为 0。
            flags.push(pname);
            if let Ok(v) = self.eval_window(st, w) {
                if pname == "CODEC" {
                    if let Value::Str(bytes) = &v {
                        codec_file = Some(bytes.clone());
                    }
                }
                event_args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        self.emit_domain(Domain::Movie, cmd.name.clone(), event_args);
        if targets.is_empty() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "MOVIEINFO 缺 LET 目标".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        }
        // 官方判定顺序 = 槽号顺序;取第一个给出的字段。
        let flag = ["PLAY", "X", "Y", "SX", "SY", "VOLUME", "CODEC"]
            .into_iter()
            .find(|name| flags.contains(name));
        let Some(flag) = flag else {
            // 一个查询字段都没给:官方什么都不写(`0x44bbe3` 直接返回)。
            return Ok(StepOutcome::Continue);
        };
        let value = match flag {
            "PLAY" => Value::Int(i64::from(self.movie_playing())),
            "X" => Value::Int(self.movie.geometry.x.round() as i64),
            "Y" => Value::Int(self.movie.geometry.y.round() as i64),
            "SX" => Value::Int(self.movie.geometry.sx.round() as i64),
            "SY" => Value::Int(self.movie.geometry.sy.round() as i64),
            "VOLUME" => Value::Int(self.movie.volume),
            _ => {
                // `CODEC`:文件不存在 = 0,MPEG-1 = 1,MPEG-2 = 2,其它 = 255。
                let code = match codec_file {
                    Some(file) => self.probe_movie_codec(&file).official_code(),
                    None => 0,
                };
                Value::Int(code)
            }
        };
        for t in &targets {
            self.write_target(st, t, &value)?;
        }
        Ok(StepOutcome::Continue)
    }

    /// 当前是否在播放:优先问播放端口(官方 `[0x801760]` 由播放器结束事件
    /// 清 0),没有端口时退回内核记录的播放意图。
    pub(super) fn movie_playing(&mut self) -> bool {
        match self.movie_host.as_mut() {
            Some(host) => host.is_playing(),
            None => self.movie.playing,
        }
    }

    /// `MOVIE[FILE, PLAY, X, Y, SX, SY, VOLUME]`(REA `0x44b134`)。
    ///
    /// 官方处理顺序(逐段锚点):
    /// 1. `X/Y/SX/SY` 先落全局(`0x44b2b0..0x44b302`),**缺省也会覆盖**:
    ///    `X/Y` 缺省 0,`SX/SY` 缺省用影片原始宽高(播放时再按窗口等比换算);
    /// 2. `FILE`:`fopen` 成功才记文件名并清播放位(`0x44b302..0x44b322`),
    ///    失败则原状返回;
    /// 3. `VOLUME` 落 `0x5b48a0` 并下发播放器(`0x44b329..`);
    /// 4. `PLAY`:1 = 开始播放,0 = 停止(并清播放位)。
    ///
    /// 解码与呈现全在 [`crate::event::MovieHost`] 之后(官方交给系统媒体栈)。
    pub(super) fn op_movie(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let args = self.eval_arg_windows(st, cmd);
        let params: &[String] = &cmd.param_names;
        // 1. 几何:每条命令都按本次参数重写(官方缺省值见文档)。
        // 官方缺省值(0x44b2b0..0x44b2fc):X/Y 缺省 0;SX/SY 缺省取影片
        // 原尺寸 ⇒ 等价于铺满窗口,内核不解码,用 0 表示「未指定」交给宿主
        // 按同一规则换算。这里没有错误吞掉,只是官方默认值。
        let geometry = crate::event::MovieGeometry {
            x: self.movie_float_arg(params, &args, "X").unwrap_or(0.0), // silent-ok: 缺省 X=0
            y: self.movie_float_arg(params, &args, "Y").unwrap_or(0.0), // silent-ok: 缺省 Y=0
            sx: self.movie_float_arg(params, &args, "SX").unwrap_or(0.0), // silent-ok: 未指定 = 铺满
            sy: self.movie_float_arg(params, &args, "SY").unwrap_or(0.0), // silent-ok: 未指定 = 铺满
        };
        self.movie.geometry = geometry;
        if let Some(host) = self.movie_host.as_mut() {
            host.set_geometry(geometry);
        }

        // 2. FILE:先验证可读(官方 `fopen`),成功才进入影片模式。
        if let Some(slot) = params.iter().position(|p| p == "FILE") {
            let file = args
                .iter()
                .find(|a| usize::from(a.slot) == slot)
                .and_then(|a| match &a.value {
                    Value::Str(bytes) if !bytes.is_empty() => Some(bytes),
                    _ => None,
                });
            if let Some(file) = file {
                match self.probe_movie_codec(file) {
                    yu_format::movie::MovieCodec::Missing => {
                        self.report(diag::recoverable(
                            code::rt::MOVIE_FILE,
                            Boundary::Script,
                            format!(
                                "MOVIE 影片不存在或不可读:{}",
                                String::from_utf8_lossy(file)
                            ),
                        ));
                    }
                    _ => {
                        self.movie.file = Some(file.clone());
                        self.movie.playing = false;
                        if self.movie_host.is_some() {
                            // 只有装了播放后端才把整份影片读进内存(官方
                            // `MOVIE` 也是此时打开文件);无后端时零额外 IO。
                            let bytes = self.open_asset(file);
                            let result = match (self.movie_host.as_mut(), bytes) {
                                (Some(host), Some(bytes)) => host.open(file, &bytes),
                                _ => Err(diag::recoverable(
                                    code::rt::MOVIE_FILE,
                                    Boundary::Script,
                                    format!(
                                        "MOVIE 影片读取失败:{}",
                                        String::from_utf8_lossy(file)
                                    ),
                                )),
                            };
                            if let Err(d) = result {
                                let d = self.locate(d);
                                self.report(d);
                            }
                        }
                    }
                }
            }
        }

        // 3. VOLUME
        if let Some(volume) = self.movie_int_arg(params, &args, "VOLUME") {
            self.movie.volume = volume;
            if let Some(host) = self.movie_host.as_mut() {
                host.set_volume(volume);
            }
        }

        // 4. PLAY
        if let Some(play) = self.movie_int_arg(params, &args, "PLAY") {
            let playing = play != 0;
            self.movie.playing = playing;
            if let Some(host) = self.movie_host.as_mut() {
                if let Err(d) = host.set_playing(playing) {
                    let d = self.locate(d);
                    self.report(d);
                }
            }
        }
        self.emit_domain(Domain::Movie, cmd.name.clone(), args);
        Ok(StepOutcome::Continue)
    }

    /// `MOVIEEND[]`(REA `0x44b9d8`):停止并退出影片模式。
    ///
    /// 官方清文件名(`0x7fab00`)、播放位(`0x801760`),并把音量重置为 256
    /// (`0x44ba0f`);几何(X/Y/SX/SY)**不清**。
    pub(super) fn op_movieend(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let args = self.eval_arg_windows(st, cmd);
        self.emit_domain(Domain::Movie, cmd.name.clone(), args);
        if let Some(host) = self.movie_host.as_mut() {
            host.close();
        }
        self.movie.file = None;
        self.movie.playing = false;
        self.movie.volume = 256;
        Ok(StepOutcome::Continue)
    }

    /// `YMV[ID, IDNO, FILE, FRAME]`(REA 0.495 `0x458ee0`)。
    ///
    /// 官方处理体在命令内完成三件事:按 `ID[.IDNO]` 取层(`0x4141e4`/
    /// `0x414559`)、`FILE` 存在时解析 `YSMV` 并把影片结构挂到 `[层+0xa0]`
    /// (`0x459094`)、`FRAME` 存在时换帧(`0x45934c`)。分层后:容器解析留在
    /// VM(它持数据源),结果经 [`crate::event::GraphicsHost::note_movie`]
    /// 同步推给图形域;图层查找/帧钳位/纹理引用归 `yu-gfx`。域事件照发,
    /// 事件契约(脚本原始参数)不变。
    ///
    /// 影片资产缺失或不是 `YSMV` 时只记可恢复诊断,不阻断脚本:官方在
    /// `FILE` 装载失败时走错误弹窗路径(`0x45905e`),图层保持原状。
    pub(super) fn op_ymv(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        let args = self.eval_arg_windows(st, cmd);
        // `FILE` 槽按工程 ysc 参数名定位,不写死槽号。
        let file_slot = self
            .tables()
            .boot
            .yscm
            .command(cmd.opcode)
            .and_then(|c| c.params.iter().position(|p| p.name == "FILE"))
            .map(|i| i as u8);
        if let Some(slot) = file_slot {
            let key = args
                .iter()
                .find(|a| a.slot == slot)
                .and_then(|a| match &a.value {
                    Value::Str(bytes) if !bytes.is_empty() => Some(bytes.clone()),
                    _ => None,
                });
            if let Some(key) = key {
                match self.resolve_movie_meta(&key) {
                    Some(info) => {
                        if let Some(host) = self.graphics_host.as_mut() {
                            host.note_movie(&key, info);
                        }
                    }
                    None => {
                        self.report(diag::recoverable(
                            code::rt::MOVIE_ASSET,
                            Boundary::Script,
                            format!(
                                "YMV 影片 `{}` 不可用(资产缺失或非 YSMV);图层保持原状",
                                String::from_utf8_lossy(&key)
                            ),
                        ));
                    }
                }
            }
        }
        self.emit_domain(Domain::Movie, cmd.name.clone(), args);
        Ok(StepOutcome::Continue)
    }

    /// 按给定路径读资产字节:工程包优先,散盘(`aux`)作备。
    ///
    /// `CGINFO[SX/SY]` 的图像头探测、`YMV[FILE]` 的容器解析与独立运行面的
    /// `TEXT[FILE]` 共用这一步;候选序(扩展名)由调用方决定,不在这里猜。
    pub(crate) fn open_asset(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        let lookup = yu_vfs::LookupKey::new(path, yu_vfs::Codec::Cp932).ok()?;
        match self.src.open(&lookup) {
            Ok(bytes) => Some(bytes),
            Err(_) => self.aux.as_ref().and_then(|aux| aux.open(&lookup).ok()),
        }
    }

    /// 只读资产前缀(`max` 字节);用于图像头探测。
    ///
    /// 数据源实现了 [`yu_vfs::DataSource::open_prefix`] 时可以只做前缀 IO;
    /// 默认实现回退整读,语义与 [`Self::open_asset`] 一致。
    pub(super) fn open_asset_prefix(&mut self, path: &[u8], max: usize) -> Option<Vec<u8>> {
        let lookup = yu_vfs::LookupKey::new(path, yu_vfs::Codec::Cp932).ok()?;
        match self.src.open_prefix(&lookup, max) {
            Ok(bytes) => Some(bytes),
            Err(_) => self
                .aux
                .as_ref()
                .and_then(|aux| aux.open_prefix(&lookup, max).ok()),
        }
    }

    /// 解析 `YMV[FILE]` 的影片元数据(只读容器头与三张表,不复制平面)。
    ///
    /// 候选序:`原样` → `原字节 + ".ymv"`(脚本两种写法都在语料里出现:
    /// `es_Sprite` 显式带 `.ymv`,`演出機能.txt` 的 `\SP.ANMV(sample2)` 由框架补)。
    /// 结果(含失败)缓存,避免重复读 6 MB 包内文件。
    pub(super) fn resolve_movie_meta(&mut self, key: &[u8]) -> Option<crate::event::MovieInfo> {
        if let Some(hit) = self.movie_meta_cache.get(key) {
            return *hit;
        }
        let mut candidates: Vec<Vec<u8>> = vec![key.to_vec()];
        let mut with_ext = key.to_vec();
        with_ext.extend_from_slice(b".ymv");
        candidates.push(with_ext);
        let mut found = None;
        'probe: for path in candidates {
            let Some(bytes) = self.open_asset(&path) else {
                continue;
            };
            match yu_format::ymv::Ymv::parse(&bytes, &path) {
                Ok(ymv) => {
                    let info = ymv.info();
                    found = Some(crate::event::MovieInfo {
                        frames: info.frames,
                        width: info.width,
                        height: info.height,
                        fps: info.fps,
                        has_alpha: info.has_alpha,
                    });
                    break 'probe;
                }
                Err(err) => {
                    // 找到文件但不是合法 YSMV:把解析诊断带出来(不再试扩展名)。
                    self.report(diag::recoverable(
                        code::rt::MOVIE_ASSET,
                        Boundary::Script,
                        format!(
                            "`{}`:{} {}",
                            String::from_utf8_lossy(&path),
                            err.code,
                            err.detail
                        ),
                    ));
                    break 'probe;
                }
            }
        }
        self.movie_meta_cache.insert(key.to_vec(), found);
        found
    }

    /// 解析 `CG[FILE]` 的像素尺寸:按工程 `yscfg` 扩展名序尝试 `.png/.bmp/
    /// .jpg/.gif`,只读图像头。结果(含失败)缓存,避免每帧重读。
    ///
    /// 候选序与渲染器 `TextureStore::candidates` **同口径**:键已带扩展名时
    /// 先按原样查,再按探测序补扩展名。两处不一致会造成「渲染器画得出、
    /// VM 量不到尺寸」:按钮的 `@BT.SX/SY` 来自 `CGINFO[SX/SY]`,为 0 时
    /// 命中矩形退化成点,点击永远不生效。
    ///
    /// 这是 [`crate::image`] 的唯一调用点;尺寸语义来自引擎 `CGINFO[SX/SY]`
    /// = 已装载纹理尺寸,证据与分层说明见 DIV-GFX-024 / EVID-J3-03。
    pub(super) fn resolve_asset_size(&mut self, key: &[u8]) -> Option<(u32, u32)> {
        if let Some(hit) = self.asset_size_cache.get(key) {
            return *hit;
        }
        let mut found = None;
        let ext_order = self.tables.boot.config.image_ext_order;
        let mut candidates: Vec<Vec<u8>> = Vec::new();
        if has_image_extension_with(key, self.name_text_codec()) {
            candidates.push(key.to_vec());
        }
        for &code in ext_order.iter() {
            let ext: &[u8] = match code {
                0 => break,
                1 => b".png",
                2 => b".bmp",
                3 => b".jpg",
                4 => b".gif",
                // 5 = .psb(E-mote 专用包);本内核不解其头。
                _ => continue,
            };
            let mut path = key.to_vec();
            path.extend_from_slice(ext);
            candidates.push(path);
        }
        'probe: for path in candidates {
            // 先只读文件头:PNG/BMP/GIF 的尺寸都在前 26 字节内,JPEG 走段链
            // 到 SOF。整读一张按钮贴图(实测平均 359 KB)只为取宽高是纯浪费
            // ——`CGINFO[SX/SY]` 单次因此 275µs,单帧 110 次就吃掉 30ms。
            //
            // 前缀里算不出尺寸(JPEG 段链超长/头部格式意外)时**回退整读**:
            // 行为与旧实现逐字节一致,只在能省的时候省。不这样做会让同一
            // 资产在两条路径下得出不同结果(实测 ops 6 vs 7)。
            let size = match self.open_asset_prefix(&path, IMAGE_HEADER_PROBE) {
                Some(prefix) => match crate::image::pixel_size(&prefix) {
                    Some(size) => Some(size),
                    None => self
                        .open_asset(&path)
                        .and_then(|full| crate::image::pixel_size(&full)),
                },
                None => None,
            };
            if let Some(size) = size {
                found = Some(size);
                break 'probe;
            }
        }
        self.asset_size_cache.insert(key.to_vec(), found);
        found
    }

    /// `MATH3D[INT=@...I() FLT=@...F()]`:ERIS 专用 3D→屏幕投影。
    ///
    /// 证据:官方 0.495 `FUN_00448900`(`0x448900`,sha256 `4f5eefa3…`)。
    /// 输入 `I(1..7)` 与 `F(3..30)`,输出写回 `F(31..34)`:
    /// - `I1` 图层号、`I2` 背景层号、`I4` 父层号、`I5/I6` 视口宽高、`I7` TIP;
    /// - `F3..F5` 相机 XYZ、`F6..F8` 背景 XYZ、`F9/F10` 背景中心、`F11/F12`
    ///   背景 CL、`F13/F14` 本层 XY、`F15` 本层 Z、`F16/F17` 缩放 BX/BY、
    ///   `F18/F19` 本层中心、`F20/F21` MX/MY(百分比)、`F22..F26` 父层、
    ///   `F27` `@DEFZ`、`F28` 相机附加 X;
    /// - 输出 `F31/F32` 屏幕 XY、`F33/F34` 缩放 X/Y。
    ///
    /// 公式按反编译逐项转写(`docs/divergences/DIV-GFX-046` 附录);
    /// 缩放是 `es_SPExec` 判 `SIZE<1` 的唯一来源,未实现时恒为 0,
    /// 全部精灵层会被 `es.CGACT_SIZE` 判为屏幕外(EVID-J3-03)。
    pub(super) fn op_math3d(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        if cmd.windows.len() < 2 {
            return Err(diag::fatal(
                code::rt::CMD_WINDOWS,
                Boundary::Script,
                format!(
                    "MATH3D 需要 INT+FLT 两个数组窗口,得到 {}",
                    cmd.windows.len()
                ),
            )
            .into());
        }
        let mut int_var = None;
        let mut flt_var = None;
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let name = self.param_name(cmd.opcode, w.rec.kind).unwrap_or_default();
            let var = self.eval_lvalue_window(st, w)?.var;
            match name.as_str() {
                "INT" => int_var = Some(var),
                "FLT" => flt_var = Some(var),
                _ => {}
            }
        }
        let (Some(int_var), Some(flt_var)) = (int_var, flt_var) else {
            return Err(diag::fatal(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "MATH3D 缺 INT/FLT 数组参数",
            )
            .into());
        };
        // 越界读取按官方 `FUN_00449b98` 语义取 0(数组元素不存在 = 0.0)。
        let rd_i = |store: &crate::value::VariableStore, i: i64| -> i64 {
            store
                .read(int_var, &[i])
                .ok()
                .and_then(|v| v.as_int())
                .unwrap_or(0)
        };
        let rd_f = |store: &crate::value::VariableStore, i: i64| -> f64 {
            store
                .read(flt_var, &[i])
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
        };
        let i1 = rd_i(&self.store, 1);
        let i2 = rd_i(&self.store, 2);
        let i4 = rd_i(&self.store, 4);
        let wsx = rd_i(&self.store, 5) as f64;
        let wsy = rd_i(&self.store, 6) as f64;
        let tip = rd_i(&self.store, 7);
        let (x, y, zx, zy) = math3d_project(i1, i2, i4, wsx, wsy, tip, &|n| rd_f(&self.store, n));
        let out_ty = self
            .store
            .shape(flt_var)
            .map(|(t, _)| t)
            .unwrap_or(crate::value::ElemType::Float);
        for (idx, value) in [(31i64, x), (32, y), (33, zx), (34, zy)] {
            let stored = if out_ty == crate::value::ElemType::Int {
                Value::Int(value.round() as i64)
            } else {
                Value::Float(value)
            };
            // 官方仅在输出元素存在时写;越界静默(同 `FUN_00448900` 尾部)。
            let _ = self.store.write(flt_var, &[idx], &stored);
        }
        Ok(StepOutcome::Continue)
    }

    /// `CGINFO/FONTINFO/WINDOWINFO`:只读查询,结果写入 `LET`。
    ///
    /// 三层来源:①手册 `cg/cginfo.html`、`font/fontinfo.html`、
    /// `window/windowinfo.html`;②ERIS 125+ 处真实使用;③查询值由
    /// [`crate::event::GraphicsHost`] 在**脚本执行期**同步提供(宿主已按
    /// 同一帧的 DomainCall 更新状态)。未安装宿主/字段未取证 ⇒ 只诊断、
    /// 不写 `LET`(不猜 0)。
    pub(super) fn op_info(
        &mut self,
        st: &mut ExecState,
        cmd: &Cmd,
        name: &str,
    ) -> Result<StepOutcome, VmError> {
        // 参数名来自工程 ysc 表(opcode → 命令声明),不写死槽号。
        //
        // **用 `cmd.param_names`(按调用点缓存一次的 `Rc<[String]>`)**,
        // 不要在这里重新收集:旧写法每次执行都 `p.name.clone()` 建成
        // 一个全新 `Vec<String>`,`CGINFO` 在菜单帧可达 1.7 万次 × 20+ 个
        // `String` = 单帧 35 万次堆分配(与 PERF-VM-HOTPATH 修的
        // `VARACT` 是同一个 bug)。
        let params: &[String] = &cmd.param_names;
        let mut targets: Vec<crate::expr::LValue> = Vec::new();
        let mut event_args: Vec<ArgValue> = Vec::new();
        let mut query_args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            if pname == "LET" {
                // 引用身份随事件走(DIV-RT-042/B1);`query` 参数面保持原
                // 契约(只含提问字段,LET 是答案目标);写回仍由 query 结果
                // 经 write_target 完成。
                let lv = self.eval_lvalue_window(st, w)?;
                let var = self.canonical_var(lv.var);
                if let Ok(v) = self.read_var(st, var, &lv.indices) {
                    event_args.push(ArgValue::with_ref(w.rec.kind, v, var));
                }
                targets.push(lv);
                continue;
            }
            if let Ok(v) = self.eval_window(st, w) {
                event_args.push(ArgValue::new(w.rec.kind, v.clone()));
                query_args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        // 事件流仍记录命令事实(trace/golden 需要);宿主会忽略 INFO 本身,
        // 真正的读取走下面的 query。
        let domain = if name == "WINDOWINFO" {
            Domain::Window
        } else {
            Domain::Graphics
        };
        self.emit_domain(domain, name.to_string(), event_args);
        if targets.is_empty() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                format!("{name} 缺 LET 目标"),
            ));
            return Ok(StepOutcome::Continue);
        }
        let Some(mut host) = self.graphics_host.take() else {
            self.report_unimplemented(
                name,
                "未安装 GraphicsHost:INFO 查询无法在脚本执行期读取图形状态",
            );
            return Ok(StepOutcome::Continue);
        };
        // 需要资产元数据回退的字段:`SX/SY`(尺寸本身)与像素查询
        // `COLOR`/`ONMOUSE*`(先要尺寸才能建命中矩形)。其余字段保持原样。
        let wanted = query_args.iter().find_map(|a| {
            params
                .get(usize::from(a.slot))
                .map(String::as_str)
                .filter(|n| {
                    matches!(
                        *n,
                        "SX" | "SY" | "COLOR" | "ONMOUSE" | "ONMOUSE2" | "ONMOUSE3"
                    )
                })
        });
        let result = host.query(name, &query_args);
        let mut value = match result {
            Ok(v) => Some(v),
            Err(d) => {
                // `CGINFO[SX/SY/COLOR/ONMOUSE*]` 的资源尺寸:gfx 不读文件 ⇒
                // 向宿主取 `MFILE` 键,由本 VM 解析图像头后回填(DIV-GFX-024)。
                //
                // 槽号一律按工程 ysc 参数名解析:该工程表里没有 `MFILE`/`ID`
                // 时不猜序号,按“尺寸不可得”继续(仍走原诊断)。
                let mut resolved = None;
                if name == "CGINFO" && wanted.is_some() {
                    let slot_of =
                        |want: &str| params.iter().position(|p| p == want).map(|i| i as u8);
                    if let (Some(mfile_slot), Some(id_slot)) = (slot_of("MFILE"), slot_of("ID")) {
                        // 0.2xx 的图层号参数名是 `NO`(0.4xx+ 才叫 `IDNO`);
                        // 两代同名语义(见 yu-gfx `alias_legacy_layer_names`)。
                        let idno_slot = slot_of("IDNO").or_else(|| slot_of("NO"));
                        let mut ask: Vec<ArgValue> = query_args
                            .iter()
                            .filter(|a| a.slot == id_slot || Some(a.slot) == idno_slot)
                            .cloned()
                            .collect();
                        ask.push(ArgValue::new(mfile_slot, Value::Int(1)));
                        if let Ok(Value::Str(key)) = host.query("CGINFO", &ask) {
                            if !key.is_empty() {
                                // 资产**不存在**时按 0×0 回填,不报错:
                                // 官方 `CGINFO[SX/SY]` 直接读图层 `+0x78/+0x7c`
                                // 的矩形,`CG[FILE=…]` 装载失败时该矩形保持 0
                                // (与 `query_layer_size` 对 `TextureRef::None`
                                // 返回 0 同一取证)。按钮系统会主动查询
                                // `*_bt3/_bt4` 这类**派生**层名(包里只有
                                // `_on/_na/_mask`),按错误处理会刷诊断并把
                                // `LET` 留在旧值上。
                                let (w, h) = self.resolve_asset_size(&key).unwrap_or((0, 0)); // silent-ok: 资产缺失 ⇒ 官方矩形 0
                                host.note_asset_size(&key, w, h);
                                resolved = match wanted {
                                    Some("SX") => Some(Value::Int(i64::from(w))),
                                    Some("SY") => Some(Value::Int(i64::from(h))),
                                    // 像素查询:尺寸回填后**重试原查询**
                                    // (命中矩形与纹理坐标都由尺寸推出)。
                                    _ => host.query(name, &query_args).ok(),
                                };
                            }
                        }
                    }
                }
                match resolved {
                    Some(v) => Some(v),
                    None => {
                        self.graphics_host = Some(host);
                        self.report(d);
                        return Ok(StepOutcome::Continue);
                    }
                }
            }
        };
        self.graphics_host = Some(host);
        if let Some(v) = value.take() {
            for t in &targets {
                self.write_target(st, t, &v)?;
            }
        }
        Ok(StepOutcome::Continue)
    }

    /// `SOUNDINFO`:只读查询,结果写入 `LET`。
    ///
    /// 证据(0.495 `0x44f4bc` 原始汇编):
    /// - `LET` 目标由 VM 持左值 → [`crate::event::SoundHost::query`] → 写回;
    /// - `EXIST..PITCH`(槽 1..7)各带独立 `ret`,按固定顺序**首个命中即返回**;
    /// - 这 7 个都不在请求里时,其余字段连续执行、最后写入者胜出;
    /// - 缺层时引擎把 LET 清零(字符串目标写空)。
    pub(super) fn op_soundinfo(&mut self, st: &mut ExecState, cmd: &Cmd) -> Result<StepOutcome, VmError> {
        // 参数名表随调用点缓存一次(`fill_cmd` 已存进 `cmd.param_names`);
        // 旧写法每次执行都重建整张 `Vec<String>`(与 `CGINFO` 同一个 bug)。
        let params: &[String] = &cmd.param_names;
        let mut targets: Vec<crate::expr::LValue> = Vec::new();
        let mut event_args: Vec<ArgValue> = Vec::new();
        let mut query_args: Vec<ArgValue> = Vec::new();
        for w in &cmd.windows {
            if w.rec.length == 0 {
                continue;
            }
            let pname = params
                .get(usize::from(w.rec.kind))
                .map(String::as_str)
                .unwrap_or("");
            if pname == "LET" {
                // 引用身份随事件走(DIV-RT-042/B1);`query` 参数面保持原
                // 契约(LET 是答案目标,不进提问字段)。
                let lv = self.eval_lvalue_window(st, w)?;
                let var = self.canonical_var(lv.var);
                if let Ok(v) = self.read_var(st, var, &lv.indices) {
                    event_args.push(ArgValue::with_ref(w.rec.kind, v, var));
                }
                targets.push(lv);
                continue;
            }
            if let Ok(v) = self.eval_window(st, w) {
                event_args.push(ArgValue::new(w.rec.kind, v.clone()));
                query_args.push(ArgValue::new(w.rec.kind, v));
            }
        }
        self.emit_domain(Domain::Sound, "SOUNDINFO", event_args);
        if targets.is_empty() {
            self.report(diag::recoverable(
                code::rt::CMD_SLOT,
                Boundary::Script,
                "SOUNDINFO 缺 LET 目标".to_string(),
            ));
            return Ok(StepOutcome::Continue);
        }
        let Some(mut host) = self.sound_host.take() else {
            self.report_unimplemented(
                "SOUNDINFO",
                "未安装 SoundHost:声音查询无法在脚本执行期读取层状态",
            );
            return Ok(StepOutcome::Continue);
        };
        let result = host.query("SOUNDINFO", &query_args);
        self.sound_host = Some(host);
        match result {
            Ok(v) => {
                for t in &targets {
                    self.write_target(st, t, &v)?;
                }
            }
            Err(d) => self.report(d),
        }
        Ok(StepOutcome::Continue)
    }
}

#[cfg(test)]
mod math3d_tests {
    use super::helpers::math3d_project;

    fn sample(fill: impl Fn(&mut [f64])) -> Vec<f64> {
        let mut f = vec![0.0; 40];
        f[3] = 0.0;
        f[4] = 0.0;
        f[5] = 0.0;
        f[6] = 0.0;
        f[7] = 0.0;
        f[8] = 200.0;
        f[9] = 400.0;
        f[10] = 300.0;
        f[13] = 0.0;
        f[14] = 0.0;
        f[15] = 200.0;
        f[16] = 100.0;
        f[17] = 100.0;
        f[18] = 0.0;
        f[19] = 0.0;
        f[20] = 100.0;
        f[21] = 100.0;
        f[27] = 200.0;
        fill(&mut f);
        f
    }

    #[test]
    fn background_is_centered_and_unscaled() {
        let f = sample(|_| {});
        let (x, y, zx, zy) = math3d_project(2, 2, 0, 800.0, 600.0, 0, &|n| f[n as usize]);
        assert_eq!((x, y), (0.0, 0.0));
        assert_eq!((zx, zy), (100.0, 100.0));
    }

    #[test]
    fn sprite_uses_layer_offset_center_and_zoom() {
        let f = sample(|f| {
            f[13] = 100.0; // layer X
            f[14] = 50.0; // layer Y
            f[18] = 200.0; // center X
            f[19] = 150.0; // center Y
        });
        let (x, y, zx, zy) = math3d_project(6, 2, 0, 800.0, 600.0, 0, &|n| f[n as usize]);
        // 背景中心 (400,300) + (图层XY - 图层中心)
        assert_eq!((x, y), (300.0, 200.0));
        assert_eq!((zx, zy), (100.0, 100.0));
    }

    #[test]
    fn tip_branch_skips_perspective() {
        let f = sample(|f| {
            f[13] = 10.0;
            f[14] = 20.0;
            f[18] = 200.0;
            f[19] = 150.0;
        });
        let (x, y, zx, zy) = math3d_project(6, 2, 0, 800.0, 600.0, 1, &|n| f[n as usize]);
        assert_eq!((x, y), (10.0 - 200.0, 20.0 - 150.0));
        assert_eq!((zx, zy), (100.0, 100.0));
    }
}

