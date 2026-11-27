//! 媒体与只读信息:MOVIE/YMV/INFO/SOUNDINFO。
//!
//! 由 `tests/fixture.rs` 拆分而来;测试名与断言不变。

use super::common::*;

/// 造一个最小 `YSMV` 容器(3×2、2 帧、30fps、带 alpha 标志)。
///
/// 段内容不是真 JPEG(VM 只读头部与三张表,不解像素),但布局与 XOR 与
/// 官方产物一致 —— 渲染端的解像素由 `yu-render-core` 的同名测试覆盖。
fn ymv_bytes() -> Vec<u8> {
    let frames = 2usize;
    let tables_end = 0x20 + frames * 12;
    let color = vec![0xAA; 5];
    let alpha = vec![0x55; 3];
    let mut out = Vec::new();
    out.extend_from_slice(b"YSMV");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(frames as u32).to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&30u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut off = tables_end;
    for _ in 0..frames {
        out.extend_from_slice(&(off as u32).to_le_bytes());
        off += color.len() + alpha.len();
    }
    for _ in 0..frames {
        out.extend_from_slice(&(color.len() as u32).to_le_bytes());
    }
    for _ in 0..frames {
        out.extend_from_slice(&(alpha.len() as u32).to_le_bytes());
    }
    for _ in 0..frames {
        out.extend_from_slice(&color);
        out.extend_from_slice(&alpha);
    }
    out
}

/// 记录 `note_movie` 调用的图形宿主(其余能力默认空实现)。
struct MovieHost {
    noted: std::rc::Rc<std::cell::RefCell<Vec<(Vec<u8>, yu_rt::event::MovieInfo)>>>,
}

impl yu_rt::event::GraphicsHost for MovieHost {
    fn apply_event(&mut self, _ev: &yu_rt::VmEvent) {}

    fn query(
        &mut self,
        command: &str,
        args: &[yu_rt::event::ArgValue],
    ) -> Result<Value, yu_rt::Diag> {
        // `YMVINFO[ID, IDNO, NUM]`:按契约回 0(层/影片不存在)。
        if command == "YMVINFO" {
            let _ = args;
            return Ok(Value::Int(0));
        }
        Err(yu_rt::diag::unimplemented(
            "rt.cmd.unimplemented",
            yu_log::Boundary::Script,
            format!("测试宿主不处理 {command}"),
        ))
    }

    fn note_movie(&mut self, key: &[u8], info: yu_rt::event::MovieInfo) {
        self.noted.borrow_mut().push((key.to_vec(), info));
    }
}

/// `YMV[FILE]`:`VM` 解析容器头 → `note_movie` 推给图形域 → 照发 `Movie`
/// 域事件;紧随的 `YMVINFO[NUM LET]` 走 `host.query` 并写回 `LET`。
#[test]
fn ymv_pushes_movie_metadata_and_ymvinfo_writes_let() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(-1),
    }];
    // 真实参数名(默认表里三条影片命令是空参数)。
    let set_params =
        |b: &mut ProjectBuilder, name: &'static str, params: Vec<(&'static str, u8, u8)>| {
            let slot = b
                .commands
                .iter_mut()
                .find(|c| c.0 == name)
                .expect("fixture command present");
            slot.1 = params;
        };
    set_params(
        &mut b,
        "YMV",
        vec![
            ("ID", 1, 0),
            ("IDNO", 0, 0),
            ("FILE", 1, 0),
            ("FRAME", 0, 0),
        ],
    );
    set_params(
        &mut b,
        "YMVINFO",
        vec![
            ("ID", 1, 0),
            ("IDNO", 0, 0),
            ("LET", 0, 0),
            ("NUM", 0, 0),
            ("SX", 0, 0),
            ("SY", 0, 0),
            ("FPS", 0, 0),
        ],
    );
    let op_ymv = b.commands.iter().position(|c| c.0 == "YMV").unwrap() as u8;
    let op_ymvinfo = b.commands.iter().position(|c| c.0 == "YMVINFO").unwrap() as u8;
    let op_cg = b.commands.iter().position(|c| c.0 == "CG").unwrap() as u8;
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // CG[ID="MV"]:建一个空图层(gfx 侧 YMV 需要目标层存在)。
        GroupSpec::new(op_cg).window(0, ex::s("MV")),
        // YMV[ID="MV" FILE="pac/sample2.ymv"]
        GroupSpec::new(op_ymv)
            .window(0, ex::s("MV"))
            .window(2, ex::s("pac/sample2.ymv")),
        // YMVINFO[ID="MV" NUM=1 LET=@a]
        GroupSpec::new(op_ymvinfo)
            .window(0, ex::s("MV"))
            .window(2, ex::r#ref(0x40, sys))
            .window(3, ex::i8_(1)),
        GroupSpec::new(OP_END),
    ]];
    let noted = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = boot_with_files(b, &[(b"pac/sample2.ymv", &ymv_bytes())]);
    vm.set_graphics_host(Box::new(MovieHost {
        noted: noted.clone(),
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    let noted = noted.borrow();
    assert_eq!(noted.len(), 1, "note_movie 应调用一次");
    assert_eq!(noted[0].0, b"pac/sample2.ymv".to_vec());
    assert_eq!(noted[0].1.frames, 2);
    assert_eq!((noted[0].1.width, noted[0].1.height), (3, 2));
    assert_eq!(noted[0].1.fps, 30);
    assert!(noted[0].1.has_alpha);

    // 测试宿主对 `YMVINFO` 应答 0 ⇒ LET 必须写成 0(官方缺层/缺片语义)。
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(0)
    );
    // 事件流仍记录 Movie 域命令(契约不变)。
    assert!(vm
        .events()
        .iter()
        .any(|e| matches!(e, yu_rt::VmEvent::DomainCall { domain, .. } if *domain == yu_rt::event::Domain::Movie)));
}

/// `YMVEND[ID]` 只有释放语义,由图形域消费(官方 `0x459a64`):
/// 发 Movie 域事件,但**不得**再按「域命令未接线」挂号。
#[test]
fn ymvend_is_consumed_by_graphics_domain() {
    let mut b = ProjectBuilder::new();
    let op_ymvend = {
        let slot = b
            .commands
            .iter_mut()
            .find(|c| c.0 == "YMVEND")
            .expect("fixture command present");
        slot.1 = vec![("ID", 1, 0), ("IDNO", 0, 0)];
        b.commands
            .iter()
            .position(|c| c.0 == "YMVEND")
            .unwrap() as u8
    };
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(op_ymvend).window(0, ex::s("MV")),
        GroupSpec::new(OP_END),
    ]];
    let noted = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = boot_with_files(b, &[]);
    vm.set_graphics_host(Box::new(MovieHost {
        noted: noted.clone(),
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let diags = vm.take_diags();
    assert!(
        !diags
            .iter()
            .any(|d| d.code.as_ref() == "rt.cmd.unimplemented"),
        "YMVEND 已由图形域消费,不能再挂未实现: {diags:?}"
    );
    assert!(
        vm.events()
            .iter()
            .any(|e| matches!(e, yu_rt::VmEvent::DomainCall { domain, .. } if *domain == yu_rt::event::Domain::Movie)),
        "YMVEND 仍须发 Movie 域事件"
    );
}

// ------------------------------------------------------------------ MOVIE

/// 记录 `MovieHost` 调用的假后端(解码不在内核,这里只验证端口契约)。
#[derive(Default)]
struct RecordingMovieHost {
    opened: Vec<Vec<u8>>,
    /// 传进来的影片字节数(验证端口拿到的是 VM 读出的完整内容)。
    opened_bytes: Vec<usize>,
    playing: Vec<bool>,
    volumes: Vec<i64>,
    geometry: Option<yu_rt::event::MovieGeometry>,
    closed: u32,
    /// 后端回报的播放状态(官方 `[0x801760]` 由播放器结束事件清 0)。
    report_playing: Option<bool>,
}

impl yu_rt::event::MovieHost for RecordingMovieHost {
    fn open(&mut self, file: &[u8], bytes: &[u8]) -> Result<(), yu_rt::Diag> {
        self.opened.push(file.to_vec());
        self.opened_bytes.push(bytes.len());
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) -> Result<(), yu_rt::Diag> {
        self.playing.push(playing);
        Ok(())
    }

    fn set_volume(&mut self, volume: i64) {
        self.volumes.push(volume);
    }

    fn set_geometry(&mut self, geometry: yu_rt::event::MovieGeometry) {
        self.geometry = Some(geometry);
    }

    fn is_playing(&mut self) -> bool {
        self.report_playing.unwrap_or(false)
    }

    fn close(&mut self) {
        self.closed += 1;
    }
}

fn movie_params(b: &mut ProjectBuilder, name: &'static str, params: Vec<(&'static str, u8, u8)>) {
    let slot = b
        .commands
        .iter_mut()
        .find(|c| c.0 == name)
        .expect("fixture command present");
    slot.1 = params;
}

/// `MOVIE` 族:状态机 + `MOVIEINFO` 查询 + 端口调用(官方 `0x44b134` /
/// `0x44b9d8` / `0x44ba2c`)。解码不在内核,这里只钉语义与端口契约。
#[test]
fn movie_state_machine_and_info_queries() {
    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 1,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        },
        VarSpec {
            kind: 1,
            group: 0,
            script_id: 0,
            var_id: sys + 2,
            value_type: 1,
            dims: vec![],
            init: Init::Int(-1),
        },
    ];
    movie_params(
        &mut b,
        "MOVIE",
        vec![
            ("FILE", 1, 0),
            ("PLAY", 0, 0),
            ("X", 0, 0),
            ("Y", 0, 0),
            ("SX", 0, 0),
            ("SY", 0, 0),
            ("VOLUME", 0, 0),
        ],
    );
    movie_params(
        &mut b,
        "MOVIEINFO",
        vec![
            ("PLAY", 0, 0),
            ("X", 0, 0),
            ("Y", 0, 0),
            ("SX", 0, 0),
            ("SY", 0, 0),
            ("VOLUME", 0, 0),
            ("CODEC", 0, 0),
            ("LET", 0, 0),
        ],
    );
    let (op_movie, op_info) = (opcode_of(&b, "MOVIE"), opcode_of(&b, "MOVIEINFO"));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        // MOVIEINFO[CODEC="pac/op_movie.mpg" LET=@a]
        // 回归(B14):`es_MoviePlay.yst` 在 `MOVIE[FILE]` **之前**用 CODEC
        // 预检参数文件;官方 `0x44bac9` 探测的是 CODEC 参数本身,不是当前
        // 已打开影片。旧实现读 `movie.file`(此时为空)恒得 0,宏路径随即
        // `IFBREAK` 放弃播放。
        GroupSpec::new(op_info)
            .window(6, ex::s("pac/op_movie.mpg"))
            .window(7, ex::r#ref(0x40, sys)),
        // MOVIE[FILE="pac/op_movie.mpg" PLAY=1 X=10 SY=120 VOLUME=128]
        GroupSpec::new(op_movie)
            .window(0, ex::s("pac/op_movie.mpg"))
            .window(1, ex::i8_(1))
            .window(2, ex::i8_(10))
            .window(5, ex::i8_(120))
            .window(6, ex::i16_(128)),
        // MOVIEINFO[X=1 LET=@b]
        GroupSpec::new(op_info)
            .window(1, ex::i8_(1))
            .window(7, ex::r#ref(0x40, sys + 1)),
        // MOVIEINFO[VOLUME=1 LET=@c]
        GroupSpec::new(op_info)
            .window(5, ex::i8_(1))
            .window(7, ex::r#ref(0x40, sys + 2)),
        GroupSpec::new(OP_END),
    ]];
    // 合成 MPEG-1 program stream 头(官方 `op_movie.mpg` 的真实前 5 字节)。
    let mut vm = boot_with_files(
        b,
        &[(b"pac/op_movie.mpg", &[0x00, 0x00, 0x01, 0xBA, 0x21][..])],
    );
    vm.set_movie_host(Box::new(RecordingMovieHost::default()));

    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(vm.movie_mode(), "FILE 装载成功后进入影片模式");
    let state = vm.movie_state();
    assert_eq!(state.file.as_deref(), Some(&b"pac/op_movie.mpg"[..]));
    assert!(state.playing);
    assert_eq!(state.volume, 128);
    // 缺省 SX/SY 记为 0(未指定,宿主按影片原尺寸铺满);X=10、SY=120 原值。
    assert_eq!(state.geometry.x, 10.0);
    assert_eq!(state.geometry.y, 0.0);
    assert_eq!(state.geometry.sx, 0.0);
    assert_eq!(state.geometry.sy, 120.0);

    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys), &[])
            .unwrap(),
        Value::Int(1),
        "CODEC:MPEG-1 = 1"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 1), &[])
            .unwrap(),
        Value::Int(10),
        "MOVIEINFO[X] 读全局"
    );
    assert_eq!(
        vm.store()
            .read(VarRef::new(VarSpace::At, sys + 2), &[])
            .unwrap(),
        Value::Int(128),
        "MOVIEINFO[VOLUME]"
    );
}

/// `MOVIEEND` 清影片模式/播放位、音量重置 256,几何保留(官方 `0x44b9d8`),
/// 并把 `close` 传给端口。
#[test]
fn movieend_clears_mode_and_resets_volume() {
    let mut b = ProjectBuilder::new();
    movie_params(
        &mut b,
        "MOVIE",
        vec![
            ("FILE", 1, 0),
            ("PLAY", 0, 0),
            ("SX", 0, 0),
            ("VOLUME", 0, 0),
        ],
    );
    let (op_movie, op_end) = (opcode_of(&b, "MOVIE"), opcode_of(&b, "MOVIEEND"));
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(op_movie)
            .window(0, ex::s("pac/op_movie.mpg"))
            .window(1, ex::i8_(1))
            .window(2, ex::i16_(320))
            .window(3, ex::i16_(200)),
        GroupSpec::new(op_end),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot_with_files(
        b,
        &[(b"pac/op_movie.mpg", &[0x00, 0x00, 0x01, 0xBA, 0x21][..])],
    );
    vm.set_movie_host(Box::new(RecordingMovieHost::default()));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!vm.movie_mode(), "MOVIEEND 退出影片模式");
    let state = vm.movie_state();
    assert!(state.file.is_none());
    assert!(!state.playing);
    assert_eq!(state.volume, 256, "音量重置 256(0x44ba0f)");
    assert_eq!(state.geometry.sx, 320.0, "几何不清(官方只清文件名/播放位)");
}

/// `MOVIE[FILE]` 指向不存在的文件:不进影片模式、留诊断(官方 `fopen` 失败
/// 时原状返回)。
#[test]
fn movie_missing_file_keeps_state_and_reports() {
    let mut b = ProjectBuilder::new();
    movie_params(&mut b, "MOVIE", vec![("FILE", 1, 0)]);
    let op_movie = opcode_of(&b, "MOVIE");
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    b.scripts = vec![vec![
        GroupSpec::new(op_movie).window(0, ex::s("pac/nope.mpg")),
        GroupSpec::new(OP_END),
    ]];
    let mut vm = boot(b);
    let _ = vm.tick_at(0, &no_input());
    assert!(!vm.movie_mode(), "文件不存在不得进入影片模式");
    assert!(vm.movie_state().file.is_none());
    assert!(
        vm.events()
            .iter()
            .any(|e| matches!(e, yu_rt::VmEvent::Diag { .. })),
        "缺文件必须留诊断"
    );
}

#[derive(Default)]
struct MockHostState {
    events: Vec<String>,
    queries: Vec<(String, Vec<u8>)>,
}

struct MockHost {
    state: std::rc::Rc<std::cell::RefCell<MockHostState>>,
}

impl yu_rt::event::GraphicsHost for MockHost {
    fn apply_event(&mut self, ev: &yu_rt::VmEvent) {
        if let yu_rt::VmEvent::DomainCall { name, .. } = ev {
            self.state.borrow_mut().events.push(name.clone());
        }
    }

    fn query(
        &mut self,
        command: &str,
        args: &[yu_rt::event::ArgValue],
    ) -> Result<yu_rt::Value, yu_rt::Diag> {
        self.state
            .borrow_mut()
            .queries
            .push((command.to_string(), args.iter().map(|a| a.slot).collect()));
        Ok(yu_rt::Value::Int(42))
    }
}

#[test]
fn info_query_writes_let_with_synchronously_applied_state() {
    // CGINFO[ID=L A=1 LET=@v]:宿主必须在 CG 之后、CGINFO 之前看到 DomainCall,
    // 查询结果 42 写回 @v;未安装宿主时则只报 Unimplemented、不写。
    let mut b = ProjectBuilder::new();
    let v = with_int_var(&mut b, 0, 0);
    // 夹具默认 CGINFO 无参数;本测试需要 ID/A/LET 三个槽。
    if let Some((_, params)) = b.commands.iter_mut().find(|(n, _)| *n == "CGINFO") {
        *params = vec![("ID", 1, 0), ("A", 0, 3), ("LET", 0, 3)];
    }
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);
    const OP_CG: u8 = 1;
    const OP_CGINFO: u8 = 4;
    b.scripts = vec![vec![
        GroupSpec::new(OP_CG).window(0, ex::s("L")),
        GroupSpec::new(OP_CGINFO)
            .window(0, ex::s("L"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, v.id)),
        GroupSpec::new(OP_END),
    ]];

    let shared = std::rc::Rc::new(std::cell::RefCell::new(MockHostState::default()));
    let mut vm = boot(b);
    vm.set_graphics_host(Box::new(MockHost {
        state: shared.clone(),
    }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);

    assert_eq!(
        vm.store().read(v, &[]).unwrap(),
        yu_rt::Value::Int(42),
        "LET 必须写入查询结果"
    );
    let st = shared.borrow();
    assert_eq!(
        st.events,
        vec!["CG".to_string(), "CGINFO".to_string()],
        "宿主同步看到 CG 与 CGINFO 事件事实"
    );
    assert_eq!(st.queries.len(), 1);
    assert_eq!(st.queries[0].0, "CGINFO");
    // ID / A 两个参数窗口;LET 不进入查询参数。
    assert_eq!(st.queries[0].1, vec![0, 1]);
}

/// `SOUNDINFO[ID=... TIME=1 LET=@x]`：VM 持 LET 左值 → `SoundHost::query`
/// → `write_target`。用 mock 宿主验证接线本身（不依赖 `yu-sound`）。
#[test]
fn soundinfo_query_writes_let_through_host() {
    use std::cell::Cell;
    use std::rc::Rc;
    use yu_rt::event::{ArgValue, SoundHost, VmEvent};

    struct Mock {
        seen: Rc<Cell<bool>>,
    }
    impl SoundHost for Mock {
        fn apply_event(&mut self, _ev: &VmEvent) {}
        fn query(&mut self, command: &str, args: &[ArgValue]) -> Result<Value, yu_log::Diag> {
            assert_eq!(command, "SOUNDINFO");
            let names: Vec<u8> = args.iter().map(|a| a.slot).collect();
            assert_eq!(names, vec![0, 1], "ID/TIME 槽");
            self.seen.set(true);
            Ok(Value::Int(1234))
        }
    }

    let mut b = ProjectBuilder::new();
    let sys = b.sysvars.len() as u16;
    b.vars = vec![VarSpec {
        kind: 1,
        group: 0,
        script_id: 0,
        var_id: sys,
        value_type: 1,
        dims: vec![],
        init: Init::Int(-1),
    }];
    // 保留完整默认命令表（boot 要求 GOSUB 等必需命令），只给 SOUNDINFO
    // 补上参数声明；opcode 仍取工程表里的实际序号。
    let op_soundinfo = b
        .commands
        .iter()
        .position(|(name, _)| *name == "SOUNDINFO")
        .expect("SOUNDINFO in fixture table") as u8;
    let op_end = b
        .commands
        .iter()
        .position(|(name, _)| *name == "END")
        .expect("END in fixture table") as u8;
    for (name, params) in b.commands.iter_mut() {
        if *name == "SOUNDINFO" {
            *params = vec![("ID", 1, 0), ("TIME", 0, 3), ("LET", 0, 0)];
        }
    }
    let groups = vec![
        GroupSpec::new(op_soundinfo)
            .window(0, ex::s("BGM"))
            .window(1, ex::i8_(1))
            .window(2, ex::r#ref(0x40, sys)),
        GroupSpec::new(op_end),
    ];
    b.scripts = vec![groups];
    b.labels = vec![("SYSTEM_START", 0, 0, 0)];
    b.entry(0, 0);

    let mut vm = boot(b);
    let seen = Rc::new(Cell::new(false));
    vm.set_sound_host(Box::new(Mock { seen: seen.clone() }));
    let report = vm.tick_at(0, &no_input());
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(seen.get(), "SoundHost::query 未被调用");
    let v = vm.store().read(user_var(&vm, 0), &[]).unwrap();
    assert_eq!(v, Value::Int(1234));
}

// ---------------------------------------------------------------- 官方分发表槽(P0-A/DIV-RT-043)

