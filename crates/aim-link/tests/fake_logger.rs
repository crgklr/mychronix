//! `Logger` against a fake MyChron that speaks the captured protocol.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener};
use std::thread;

use aim_link::frame::{Frame, checksum};
use aim_link::{Logger, client};

const TKK_LISTING: &[u8] = include_bytes!("data/tkk-listing.bin");

/// Builds a directory listing the way the logger formats one.
fn listing(files: &[(&str, usize)]) -> Vec<u8> {
    let field = |tag: &[u8; 4], text: &str| Frame { tag: *tag, flag: 1, payload: text.as_bytes().to_vec() }.encode();
    let mut out = Vec::new();
    for (name, size) in files {
        let mut body = field(b"TMD\0", "10/10/2026");
        body.extend(field(b"TMT\0", "13:49:35"));
        body.extend(field(b"SZ\0\0", &size.to_string()));
        body.extend(field(b"N\0\0\0", name));
        out.extend(Frame { tag: *b"FLE\0", flag: 0, payload: body }.encode());
    }
    out
}

/// Serves one connection. Chunks are `chunk` bytes; the first pull of each
/// file is answered twice (a stale duplicate) to exercise the client's skip.
fn serve(listener: TcpListener, files: HashMap<String, Vec<u8>>, chunk: usize) {
    let (stream, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let send = |w: &mut std::net::TcpStream, payload: Vec<u8>| {
        w.write_all(&Frame::new(b"STCP", payload).encode()).unwrap();
    };

    let hello = Frame::read_from(&mut reader, 1 << 20).unwrap();
    assert_eq!(hello.payload, [0, 0, 0, 0, 6, 8, 0, 0]);
    send(&mut writer, vec![0, 0, 0, 0, 6, 9, 0, 0]);

    let mut current: Vec<u8> = Vec::new();
    let mut duplicated = false;
    while let Ok(req) = Frame::read_from(&mut reader, 1 << 20) {
        match (&req.tag, req.payload.len()) {
            (b"STNC", 64) => {
                let p = &req.payload;
                let command = (u16::from_le_bytes([p[8], p[9]]), u16::from_le_bytes([p[10], p[11]]));
                let path = String::from_utf8_lossy(&p[32..]).trim_end_matches('\0').to_string();
                current = match command {
                    client::cmd::READ_FILE => files[&path].clone(),
                    client::cmd::LIST_DIR if path == "0:/tkk" => TKK_LISTING.to_vec(),
                    client::cmd::LIST_DIR => {
                        let names: Vec<_> = files.iter().map(|(n, d)| (n.rsplit('/').next().unwrap(), d.len())).collect();
                        listing(&names)
                    }
                    other => panic!("unexpected command {other:?}"),
                };
                duplicated = false;
                let mut header = p.to_vec();
                header[20..24].copy_from_slice(&[0xc0, 0xff, 0, 0]);
                header[24..26].copy_from_slice(&0x0a09u16.to_le_bytes());
                send(&mut writer, header.clone());
                header[16..20].copy_from_slice(&(current.len() as u32).to_le_bytes());
                header[24..26].copy_from_slice(&0x0a11u16.to_le_bytes());
                send(&mut writer, header);
            }
            (b"STCP", 4) => {
                let offset = u32::from_le_bytes(req.payload[..4].try_into().unwrap()) as usize;
                let end = (offset + chunk).min(current.len());
                let mut reply = (offset as u32).to_le_bytes().to_vec();
                reply.extend_from_slice(&current[offset..end]);
                if offset > 0 && !duplicated {
                    // Replay the previous chunk first, as if a late retransmission arrived.
                    let prev = offset - chunk;
                    let mut stale = (prev as u32).to_le_bytes().to_vec();
                    stale.extend_from_slice(&current[prev..offset]);
                    send(&mut writer, stale);
                    duplicated = true;
                }
                send(&mut writer, reply);
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
}

fn start(files: HashMap<String, Vec<u8>>, chunk: usize) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || serve(listener, files, chunk));
    addr
}

#[test]
fn downloads_a_file_across_chunks() {
    let session: Vec<u8> = (0..10_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let addr = start(HashMap::from([("1:/mem/a_0001.xrz".to_string(), session.clone())]), 4096);
    let mut logger = Logger::connect(addr).unwrap();
    let mut calls = Vec::new();
    let got = logger.read_file("1:/mem/a_0001.xrz", |done, total| calls.push((done, total))).unwrap();
    assert_eq!(got, session);
    assert_eq!(calls, vec![(4096, 10_000), (8192, 10_000), (10_000, 10_000)]);
    assert_eq!(checksum(&got), checksum(&session));
}

#[test]
fn lists_sessions_and_reads_real_listing() {
    let files = HashMap::from([
        ("1:/mem/a_0034.xrz".to_string(), vec![1u8; 300]),
        ("1:/mem/a_0033.xrz".to_string(), vec![2u8; 200]),
        ("1:/mem/notes.txt".to_string(), vec![3u8; 10]),
    ]);
    let addr = start(files, 65_472);
    let mut logger = Logger::connect(addr).unwrap();

    let sessions = logger.sessions().unwrap();
    let names: Vec<_> = sessions.iter().map(|e| (e.name.as_str(), e.size)).collect();
    assert_eq!(names, [("a_0033.xrz", 200), ("a_0034.xrz", 300)]);

    // The track folder listing exactly as a MyChron 6 sent it.
    let tracks = logger.list("0:/tkk").unwrap();
    let tracks: Vec<_> = tracks.iter().map(|e| (e.name.as_str(), e.size, e.date.as_str())).collect();
    assert_eq!(
        tracks,
        [
            ("al-2-ll.idx", 5310, "07/09/2026"),
            ("al.ria", 339840, "07/09/2026"),
            ("ll-2-al.idx", 5310, "07/09/2026"),
            ("ll.ria", 339840, "07/09/2026"),
            ("dev.ria", 128, "10/10/2026"),
        ]
    );
}
