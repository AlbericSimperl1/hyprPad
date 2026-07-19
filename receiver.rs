use std::net::UdpSocket;

fn main() {
    // Luister op poort 1234
    let socket = UdpSocket::bind("127.0.0.1:1234").expect("Kon niet binden aan poort");
    println!("Luisteren naar binnenkomende video-pakketjes op poort 1234...");

    let mut buf = [0u8; 4096];
    let mut packet_count = 0;

    loop {
        match socket.recv_from(&mut buf) {
            Ok((amt, src)) => {
                packet_count += 1;
                // MPEG-TS chunks horen exact 1316 bytes te zijn (7 * 188)
                println!(
                    "Pakket #{} ontvangen! Grootte: {} bytes van {}",
                    packet_count, amt, src
                );
            }
            Err(e) => {
                eprintln!("Fout bij ontvangen: {}", e);
            }
        }
    }
}
