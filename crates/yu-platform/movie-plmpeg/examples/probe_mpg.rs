//! 探针:用内置 pl_mpeg 打开用户给定的 MPG,打印头信息/前几帧统计。
use std::collections::BTreeSet;
use yu_movie_plmpeg::PlmMovieHost;
use yu_rt::event::MovieHost;

fn dump_ppm(path: &str, w: u32, h: u32, rgba: &[u8]) {
    let mut out = format!("P6
{w} {h}
255
").into_bytes();
    for p in rgba.chunks_exact(4) {
        out.extend_from_slice(&p[..3]);
    }
    std::fs::write(path, out).unwrap();
}

fn main() {
    let path = std::env::args().nth(1).expect("mpg path");
    let bytes = std::fs::read(&path).expect("read");
    let mut host = PlmMovieHost::new();
    match host.open(b"probe.mpg", &bytes) {
        Ok(()) => {}
        Err(d) => {
            println!("OPEN-ERR {d:?}");
            return;
        }
    }
    println!(
        "media={:?} audio={} rate={}",
        host.media_size(),
        host.has_audio(),
        host.audio_rate()
    );
    host.set_playing(true).unwrap();
    let checkpoints = [0u64, 100, 500, 1000, 2000, 3000, 4000, 5000, 8000, 12000, 16000, 20000];
    let mut last_rgba_len = 0usize;
    let mut last_sig = 0u64;
    let mut now = 0u64;
    for ms in checkpoints {
        while now < ms {
            now += 16;
            let _ = host.frame(now);
        }
        let _ = last_rgba_len;
        let _ = last_sig;
        match host.frame(now) {
            Some(f) => {
                if ms == 1000 || ms == 4000 || ms == 8000 || ms == 12000 {
                    let out = std::env::temp_dir().join(format!("probe-{ms}.ppm"));
                    dump_ppm(&out.to_string_lossy(), f.width, f.height, &f.rgba);
                }
                let colors: BTreeSet<[u8; 4]> = f
                    .rgba
                    .chunks_exact(4)
                    .map(|p| [p[0], p[1], p[2], p[3]])
                    .collect();
                let mean: u64 = f
                    .rgba
                    .chunks_exact(4)
                    .map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2]))
                    .sum::<u64>()
                    / (f.rgba.len() as u64 / 4 * 3);
                println!(
                    "ms={ms} {}x{} colors={} mean={} playing={}",
                    f.width,
                    f.height,
                    colors.len(),
                    mean,
                    host.is_playing()
                );
            }
            None => println!("ms={ms} no-frame playing={}", host.is_playing()),
        }
    }
}
