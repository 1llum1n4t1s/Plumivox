mod support;
use anyhow::ensure;
use hyper::{Request, body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use serde_json::json;
use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::{McpClient, Result, Suite, json_response};

#[tokio::test]
async fn native_tls_verification() -> Result<()> {
    let mut suite = Suite::new("PLUMIVOX_TLS_E2E_OUTPUT_DIR", "tls")?;
    let key = "synthetic-tls-api-key-never-display";
    suite.add_secret(key.into());
    let secrets = suite.output().join("synthetic-secrets.json");
    fs::write(
        &secrets,
        json!({"twitterapi_io":{"reading":{"user_name":"Reader","api_key":key}}}).to_string(),
    )?;
    let trace = Arc::new(Mutex::new(Vec::new()));
    let mut diagnostics = Vec::new();
    for (name, leaf, trusted, success) in [
        (
            "Trusted fixture CA accepts the 127.0.0.1 IP SAN",
            "valid",
            true,
            true,
        ),
        (
            "Untrusted fixture CA is rejected before HTTP",
            "valid",
            false,
            false,
        ),
        (
            "Trusted fixture CA does not bypass hostname verification",
            "wrong",
            true,
            false,
        ),
    ] {
        // 初期化・MCP起動・検証が失敗しても、そこまでの実測を残す。
        let connections = Arc::new(AtomicUsize::new(0));
        let rejections = Arc::new(AtomicUsize::new(0));
        let result:Result<()> = async {
            let fixture=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tls");
            let certificate=CertificateDer::pem_file_iter(fixture.join(format!("{leaf}-cert.pem")))?.collect::<std::result::Result<Vec<_>,_>>()?;
            let private=PrivateKeyDer::from_pem_file(fixture.join(format!("{leaf}-key.pem")))?;
            let mut config=rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider())).with_safe_default_protocol_versions()?.with_no_client_auth().with_single_cert(certificate,private)?;
            config.alpn_protocols=vec![b"http/1.1".to_vec()];
            let acceptor=tokio_rustls::TlsAcceptor::from(Arc::new(config));
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let port=listener.local_addr()?.port();
            let (count,rejected,t)=(connections.clone(),rejections.clone(),trace.clone());
            let before=trace.lock().unwrap().len();
            let stop=tokio_util::sync::CancellationToken::new();let stopping=stop.clone();
            let _stop_guard=stop.clone().drop_guard();
            let task=tokio::spawn(async move {let mut tasks=tokio::task::JoinSet::new();loop {tokio::select!{
                _=stopping.cancelled()=>break,
                accepted=listener.accept()=>{let Ok((socket,_))=accepted else{break};count.fetch_add(1,Ordering::SeqCst);let (a,t,r)=(acceptor.clone(),t.clone(),rejected.clone());tasks.spawn(async move {
                    let Ok(tls)=a.accept(socket).await else {r.fetch_add(1,Ordering::SeqCst);return;};
                    let protocol=format!("{:?}",tls.get_ref().1.protocol_version());let alpn=tls.get_ref().1.alpn_protocol().map(|v|String::from_utf8_lossy(v).to_string());
                    let service=service_fn(move |request:Request<Incoming>|{t.lock().unwrap().push(json!({"case":name,"method":request.method().as_str(),"path":request.uri().path(),"authenticated":request.headers().get("x-api-key").and_then(|v|v.to_str().ok())==Some(key),"tlsVersion":protocol,"alpnProtocol":alpn}));async {Ok::<_,std::io::Error>(json_response(200,json!({"tweets":[{"id":"502","text":"synthetic TLS tweet"}]})))}});
                    let _=hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(tls),service).await;
                });}
            }}tasks.abort_all();while tasks.join_next().await.is_some(){} });
            let mut client=McpClient::spawn(&[format!("https://127.0.0.1:{port}"),secrets.display().to_string(),suite.output().join(format!("state-{leaf}-{trusted}")).display().to_string(),"5000".into(),if trusted{"--tls-trusted-fixture"}else{"--tls-untrusted-fixture"}.into()]).await?;
            let checked:Result<()>=async {
                let result=client.call_tool("get_tweets",json!({"account":"reading","tweetIds":["502"]})).await?;
                ensure!(!result.to_string().contains(key),"MCP exposed API key");
                let data=result.get("structuredContent").cloned().or_else(||result["content"].as_array()?.iter().find_map(|v|serde_json::from_str(v["text"].as_str()?).ok())).ok_or_else(||anyhow::anyhow!("Missing MCP content"))?;
                ensure!(connections.load(Ordering::SeqCst)>0,"TLS was not attempted");
                let trace=trace.lock().unwrap();
                if success {ensure!(result["isError"]!=true && data["tweets"][0]["id"]=="502");ensure!(trace.len()==before+1);let row=&trace[before];ensure!(row["method"]=="GET"&&row["path"]=="/twitter/tweets"&&row["authenticated"]==true);ensure!(["Some(TLSv1_2)","Some(TLSv1_3)"].contains(&row["tlsVersion"].as_str().unwrap_or("")));ensure!(row["alpnProtocol"]=="http/1.1");}
                else {ensure!(result["isError"]==true && data["error"]["code"]=="API_UNAVAILABLE");ensure!(trace.len()==before,"Invalid certificate reached HTTP handler");}Ok(())
            }.await;
            client.close().await?;ensure!(!client.stderr().contains(key));stop.cancel();tokio::time::timeout(Duration::from_secs(3),task).await??;checked
        }.await;
        diagnostics.push(json!({
            "case": name,
            "tcpConnections": connections.load(Ordering::SeqCst),
            "httpRequests": trace.lock().unwrap().iter().filter(|row| row["case"] == name).count(),
            "tlsHandshakeRejections": rejections.load(Ordering::SeqCst),
        }));
        suite.record(name, result);
    }
    suite.write_json("http-trace.json", &json!(trace.lock().unwrap().clone()))?;
    suite.write_json("diagnostics.json", &json!(diagnostics))?;
    suite.finish()
}
