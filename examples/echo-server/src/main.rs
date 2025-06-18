use orengine::io::full_buffer;
use orengine::net::{Listener, Stream, TcpListener, ToSockAddrs};
use orengine::{local_executor, run_local_future_on_all_cores};

async fn handle_stream<S: Stream>(mut stream: S) {
    loop {
        stream.poll_recv().await.expect("poll failed");

        let mut buf = full_buffer();
        let n = stream.recv(&mut buf).await.expect("recv failed");

        if n == 0 {
            break;
        }

        stream.send_all(&buf.slice(..n)).await.expect("send failed");
    }
}

async fn run_listener<L: Listener + 'static>(addr: impl ToSockAddrs<L::Addr>) {
    let mut listener = L::bind(addr).await.expect("bind failed");

    while let Ok((stream, _addr)) = listener.accept().await {
        local_executor().spawn_local(handle_stream(stream));
    }
}

fn main() {
    run_local_future_on_all_cores(|| run_listener::<TcpListener>("127.0.0.1:8080"));
}
