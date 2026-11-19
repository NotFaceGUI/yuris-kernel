//! 临时探针:用内嵌 pl_mpeg 打开实际 BRANDCALL 影片并推进播放时钟。
//! 用法:`probe_brandcall <file.mpg> [frames]`
use yu_rt::event::MovieHost; // trait
use yu_movie_plmpeg::PlmMovieHost;

fn main() {
    let path = std::env::args().nth(1).expect("mpg path");
    let steps: u64 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(120);
    let bytes = std::fs::read(&path).expect("read");
    let mut host = PlmMovieHost::new();
    match host.open(path.as_bytes(), &bytes) {
        Ok(()) => println!("open ok, {} bytes", bytes.len()),
        Err(e) => {
            println!("open FAILED: {} {}", e.code, e.detail);
            return;
        }
    }
    host.set_playing(true).unwrap();
    let mut non_black = 0usize;
    let mut frames = 0usize;
    let mut info_play = -1;
    for i in 0..steps {
        let f = host.frame(i * 33);
        if let Some(f) = f {
            frames += 1;
            if i % 10 == 0 {
                let px = f.rgba.len();
                let sum: u64 = f.rgba.iter().step_by(97).map(|&b| u64::from(b)).sum();
                if sum > 0 { non_black += 1; }
                println!("f{i}: {}x{} bytes={px} sample_sum={sum}", f.width, f.height);
            }
        }
        info_play = i64::from(host.is_playing());
    }
    println!("frames_seen={frames} non_black_samples={non_black} info_play={info_play}");
}
