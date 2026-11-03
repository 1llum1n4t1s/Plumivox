mod proxy_support;
mod support;

use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use proxy_support::{Harness, Mode, active_is, gate_count, rejected_tunnel, release_gate, state};
use serde_json::{Value, json};
use std::{fs, io::Write, sync::atomic::Ordering, time::Duration};
use support::{Result, Suite, free_port, wait_until};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

fn response(h: &Harness, raw: &Value) -> Result<Value> {
    let data = h.business(raw)?;
    ensure!(
        raw["isError"] != true,
        "Proxy operation failed: {}",
        data["error"]["code"]
    );
    Ok(data)
}
fn tool(name: &str, arguments: Value) -> Value {
    json!({"name":name,"arguments":arguments})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_shared_proxy_e2e() -> Result<()> {
    let mut suite = Suite::new(
        "PLUMIVOX_PROXY_NATIVE_E2E_OUTPUT_DIR",
        "native shared proxy",
    )?;
    let mut h = Harness::new(suite.output()).await?;
    for secret in &h.secrets {
        suite.add_secret(secret.clone());
    }
    let primary = h.settings("primary", json!({})).await?;
    let mut second = None;
    macro_rules! case {
        ($name:expr,$body:block)=>{{
            let result:Result<()>=async $body.await;
            h.ssh.set_mode(Mode::Accept);
            suite.record($name,result);
        }};
    }

    case!(
        "two MCPs share one native SSH; detach and external leave it alive",
        {
            let a = h.client_for(&primary, &[]).await?;
            let b = h.client_for(&primary, &[]).await?;
            second = Some(b);
            let before = h.ssh.attempts();
            let (one, two) = tokio::join!(
                h.success(a, "start_proxy", json!({"account":"managed"})),
                h.success(b, "start_proxy", json!({"account":"managed"}))
            );
            ensure!(
                state(&one?) == "ready" && state(&two?) == "ready",
                "Both MCPs must become ready"
            );
            ensure!(
                h.ssh.attempts() - before == 1 && h.ssh.active() == 1,
                "Concurrent MCPs did not share one SSH owner"
            );
            let mut equivalent = primary.config.clone();
            equivalent["plumivox"]["managed_proxy"]["private_key_file"] = json!(
                h.key_file
                    .parent()
                    .unwrap()
                    .join("unused")
                    .join("..")
                    .join(h.key_file.file_name().unwrap())
            );
            fs::write(&primary.file, serde_json::to_vec(&equivalent)?)?;
            h.success(b, "start_proxy", json!({"account":"managed"}))
                .await?;
            ensure!(
                h.ssh.attempts() - before == 1,
                "Equivalent key path created another owner"
            );
            fs::write(&primary.file, serde_json::to_vec(&primary.config)?)?;
            rejected_tunnel(primary.remote).await?;
            h.close_client(a).await?;
            ensure!(
                state(&h.success(b, "get_proxy_status", json!({})).await?) == "ready"
                    && h.ssh.active() == 1,
                "One MCP departure stopped the shared owner"
            );
            let requests = h.requests.load(Ordering::SeqCst);
            h.success(
                b,
                "post_tweet",
                json!({"account":"external","text":"External route stays independent"}),
            )
            .await?;
            ensure!(
                h.requests.load(Ordering::SeqCst) == requests + 1,
                "External posting request count changed"
            );
            let (raw, data) = h
                .call(b, "start_proxy", json!({"account":"external"}))
                .await?;
            ensure!(
                raw["isError"] == true && data["error"]["code"] == "PROXY_EXTERNAL",
                "External start must remain unmanaged"
            );
            ensure!(
                state(&h.success(b, "get_proxy_status", json!({})).await?) == "ready"
                    && h.ssh.active() == 1,
                "External operation stopped shared forwarding"
            );
            Ok(())
        }
    );
    case!(
        "SSH disconnect reconnects transport without another MCP start",
        {
            let b = second.context("Primary MCP was not started")?;
            let before = h.ssh.attempts();
            h.ssh.disconnect_all().await;
            wait_until(Duration::from_secs(5), || async {
                Ok(h.ssh.attempts() > before && h.ssh.active() == 1)
            })
            .await?;
            wait_until(Duration::from_secs(5), || async {
                Ok(state(&h.success(b, "get_proxy_status", json!({})).await?) == "ready")
            })
            .await?;
            ensure!(
                h.ssh.attempts() == before + 1,
                "Transport reconnected more than once"
            );
            rejected_tunnel(primary.remote).await?;
            Ok(())
        }
    );
    case!(
        "different configuration has independent daemon; stop unbinds only its own ports",
        {
            let b = second.context("Primary MCP was not started")?;
            let other = h.settings("isolated", json!({})).await?;
            let isolated = h.client_for(&other, &[]).await?;
            ensure!(
                state(
                    &h.success(isolated, "start_proxy", json!({"account":"managed"}))
                        .await?
                ) == "ready",
                "Isolated setting did not become ready"
            );
            ensure!(
                h.ssh.active() == 2,
                "Different settings did not have separate owners"
            );
            let before = h.ssh.unforwarded();
            ensure!(
                state(&h.success(b, "stop_proxy", json!({})).await?) == "stopped",
                "Stop did not report stopped"
            );
            active_is(&h.ssh, 1).await?;
            ensure!(
                h.ssh.unforwarded() == before + 1,
                "Primary reverse listener was not cancelled once"
            );
            ensure!(
                state(&h.success(isolated, "get_proxy_status", json!({})).await?) == "ready",
                "Stopping primary stopped the isolated setting"
            );
            rejected_tunnel(other.remote).await?;
            let probe = TcpListener::bind(("127.0.0.1", primary.local)).await?;
            drop(probe);
            h.success(isolated, "stop_proxy", json!({})).await?;
            active_is(&h.ssh, 0).await?;
            h.close_client(b).await?;
            second = None;
            h.close_client(isolated).await?;
            Ok(())
        }
    );
    case!(
        "cancelled readiness waiter does not cancel another MCP or shared SSH",
        {
            h.ssh.set_mode(Mode::Stall);
            let a = h.client_for(&primary, &[]).await?;
            let b = h.client_for(&primary, &[]).await?;
            let cancelled = h.clients[a]
                .start_request(
                    "tools/call",
                    tool("start_proxy", json!({"account":"managed"})),
                )
                .await?;
            let survivor = h.clients[b]
                .start_request(
                    "tools/call",
                    tool("start_proxy", json!({"account":"managed"})),
                )
                .await?;
            wait_until(Duration::from_secs(5), || async { Ok(h.ssh.stalled() > 0) }).await?;
            h.clients[a].cancel(cancelled.id).await?;
            drop(cancelled);
            // 取消後の同じMCPのstatus完了で、その待機が共有managerのmutexを解放したと確認する。
            h.success(a, "get_proxy_status", json!({})).await?;
            h.ssh.set_mode(Mode::Accept);
            ensure!(
                state(&response(
                    &h,
                    &survivor.wait_timeout(Duration::from_secs(20)).await?
                )?) == "ready",
                "Other MCP did not complete readiness"
            );
            ensure!(
                h.ssh.active() == 1,
                "Cancelling one waiter disturbed shared SSH"
            );
            h.close_client(a).await?;
            ensure!(
                state(&h.success(b, "get_proxy_status", json!({})).await?) == "ready",
                "Cancelled MCP departure stopped other client"
            );
            rejected_tunnel(primary.remote).await?;
            h.success(b, "stop_proxy", json!({})).await?;
            active_is(&h.ssh, 0).await?;
            h.close_client(b).await?;
            Ok(())
        }
    );
    case!(
        "same MCP stop cancels pending posting readiness before any API request",
        {
            h.ssh.set_mode(Mode::Stall);
            let client = h.client_for(&primary, &[]).await?;
            let before = h.requests.load(Ordering::SeqCst);
            let pending = h.clients[client]
                .start_request(
                    "tools/call",
                    tool(
                        "post_tweet",
                        json!({"account":"managed","text":"Do not transmit after stop"}),
                    ),
                )
                .await?;
            wait_until(Duration::from_secs(5), || async { Ok(h.ssh.stalled() > 0) }).await?;
            let stopping = h.clients[client]
                .start_request("tools/call", tool("stop_proxy", json!({})))
                .await?;
            let raw = pending.wait_timeout(Duration::from_secs(20)).await?;
            let data = h.business(&raw)?;
            ensure!(
                raw["isError"] == true && data["error"]["code"] == "PROXY_STOPPED",
                "Pending posting was not stopped"
            );
            ensure!(
                state(&response(
                    &h,
                    &stopping.wait_timeout(Duration::from_secs(20)).await?
                )?) == "stopped",
                "Stop did not finish"
            );
            ensure!(
                h.requests.load(Ordering::SeqCst) == before,
                "Stopped preparation transmitted a posting request"
            );
            h.ssh.set_mode(Mode::Accept);
            active_is(&h.ssh, 0).await?;
            h.close_client(client).await?;
            Ok(())
        }
    );
    case!(
        "simultaneous gated daemon launches both attach to the winning owner",
        {
            let config = h.settings("bind-race", json!({})).await?;
            let gate = h
                .output
                .join(format!("bind-race-{:016x}", rand::random::<u64>()));
            let env = vec![(
                "PLUMIVOX_PROXY_TEST_BIND_GATE".into(),
                gate.to_string_lossy().into_owned(),
            )];
            let a = h.client_for(&config, &env).await?;
            let b = h.client_for(&config, &env).await?;
            let before = h.ssh.attempts();
            let one = h.clients[a]
                .start_request(
                    "tools/call",
                    tool("start_proxy", json!({"account":"managed"})),
                )
                .await?;
            let two = h.clients[b]
                .start_request(
                    "tools/call",
                    tool("start_proxy", json!({"account":"managed"})),
                )
                .await?;
            wait_until(Duration::from_secs(5), || async {
                Ok(gate_count(&h.output, &gate)? == 2)
            })
            .await?;
            release_gate(&gate)?;
            let (one, two) = tokio::join!(
                one.wait_timeout(Duration::from_secs(20)),
                two.wait_timeout(Duration::from_secs(20))
            );
            ensure!(
                state(&response(&h, &one?)?) == "ready" && state(&response(&h, &two?)?) == "ready",
                "Both MCPs did not attach to bind winner"
            );
            ensure!(
                h.ssh.attempts() - before == 1,
                "Bind race created multiple SSH owners"
            );
            h.success(a, "stop_proxy", json!({})).await?;
            active_is(&h.ssh, 0).await?;
            ensure!(
                state(&h.success(b, "get_proxy_status", json!({})).await?) == "stopped",
                "Peer did not observe stopped daemon"
            );
            ensure!(
                state(&h.success(b, "stop_proxy", json!({})).await?) == "stopped",
                "Repeated peer stop did not settle"
            );
            h.close_client(a).await?;
            h.close_client(b).await?;
            Ok(())
        }
    );
    case!(
        "stop before daemon bind waits for pending launch and prevents late resurrection",
        {
            let config = h.settings("bind-gated", json!({})).await?;
            let gate = h
                .output
                .join(format!("bind-gate-{:016x}", rand::random::<u64>()));
            let env = vec![(
                "PLUMIVOX_PROXY_TEST_BIND_GATE".into(),
                gate.to_string_lossy().into_owned(),
            )];
            // 別MCPの未接続起動は停止後に成立し得る（DESIGN.md）。二MCP競合は直前のcaseで検証する。
            // ここはA自身の起動停止を確認し、SSH0の確認前には追加stopやcleanupを行わない。
            let a = h.client_for(&config, &env).await?;
            let pending_a = h.clients[a]
                .start_request(
                    "tools/call",
                    tool("start_proxy", json!({"account":"managed"})),
                )
                .await?;
            wait_until(Duration::from_secs(5), || async {
                Ok(gate_count(&h.output, &gate)? == 1)
            })
            .await?;
            let stopping = h.clients[a]
                .start_request("tools/call", tool("stop_proxy", json!({})))
                .await?;
            let stop_wait = stopping.wait_timeout(Duration::from_secs(20));
            tokio::pin!(stop_wait);
            ensure!(
                timeout(Duration::from_millis(150), &mut stop_wait)
                    .await
                    .is_err(),
                "Stop returned before pending daemon could bind"
            );
            release_gate(&gate)?;
            let cancelled = h.business(&pending_a.wait_timeout(Duration::from_secs(20)).await?)?;
            ensure!(
                cancelled["error"]["code"] == "PROXY_STOPPED",
                "Initial readiness was not stopped"
            );
            ensure!(
                state(&response(&h, &stop_wait.await?)?) == "stopped",
                "Stop did not settle after bind"
            );
            tokio::time::sleep(Duration::from_millis(1200)).await;
            ensure!(
                h.ssh.active() == 0,
                "Earlier waiter resurrected the explicitly stopped daemon"
            );
            h.close_client(a).await?;
            Ok(())
        }
    );
    case!(
        "same MCP retains earlier configured daemon beyond idle while using another setting",
        {
            let config = h.settings("retained", json!({})).await?;
            let client = h.client_for(&config, &[]).await?;
            ensure!(
                state(
                    &h.success(client, "start_proxy", json!({"account":"managed"}))
                        .await?
                ) == "ready",
                "Original setting did not become ready"
            );
            let mut alternate = config.config.clone();
            alternate["plumivox"]["managed_proxy"]["local_port"] = json!(free_port()?);
            alternate["plumivox"]["managed_proxy"]["remote_port"] = json!(free_port()?);
            fs::write(&config.file, serde_json::to_vec(&alternate)?)?;
            ensure!(
                state(
                    &h.success(client, "start_proxy", json!({"account":"managed"}))
                        .await?
                ) == "ready",
                "Alternate setting did not become ready"
            );
            tokio::time::sleep(Duration::from_millis(2300)).await;
            ensure!(
                h.ssh.active() == 2,
                "Configuration switch detached earlier in-flight proxy"
            );
            rejected_tunnel(config.remote).await?;
            h.success(client, "stop_proxy", json!({})).await?;
            active_is(&h.ssh, 1).await?;
            fs::write(&config.file, serde_json::to_vec(&config.config)?)?;
            ensure!(
                state(
                    &h.success(client, "start_proxy", json!({"account":"managed"}))
                        .await?
                ) == "ready"
                    && h.ssh.active() == 1,
                "Earlier owner was not retained"
            );
            h.success(client, "stop_proxy", json!({})).await?;
            h.close_client(client).await?;
            active_is(&h.ssh, 0).await?;
            Ok(())
        }
    );
    case!(
        "unanswered unforward cannot hang stop and active partial CONNECT is closed",
        {
            let config = h.settings("forced-stop", json!({})).await?;
            let client = h.client_for(&config, &[]).await?;
            h.success(client, "start_proxy", json!({"account":"managed"}))
                .await?;
            let mut socket = TcpStream::connect(("127.0.0.1", config.remote)).await?;
            socket.write_all(b"CONNECT x.com:443 HTTP/1.1\r\n").await?;
            wait_until(Duration::from_secs(5), || async {
                Ok(
                    h.success(client, "get_proxy_status", json!({})).await?["activeConnections"]
                        == 1,
                )
            })
            .await?;
            let before_stall = h.ssh.stalled_unforward_requests(config.remote);
            h.ssh.set_mode(Mode::UnforwardStall);
            let stop_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            tokio::time::timeout_at(stop_deadline, async {
                let stopping = h.clients[client]
                    .start_request("tools/call", tool("stop_proxy", json!({})))
                    .await?;
                wait_until(Duration::from_secs(5), || async {
                    Ok(h.ssh.stalled_unforward_requests(config.remote) > before_stall)
                })
                .await
                .context("Target reverse port did not receive the stalled cancellation")?;
                let raw = stopping.wait_timeout(Duration::from_secs(5)).await?;
                ensure!(
                    state(&response(&h, &raw)?) == "stopped",
                    "Stop did not report stopped"
                );
                Ok::<(), anyhow::Error>(())
            })
            .await
            .context("Unanswered unforward hung stop")??;
            ensure!(
                h.ssh.stalled_unforward_requests(config.remote) == before_stall + 1,
                "Target cancellation was not delivered exactly once in stall mode"
            );
            let mut bytes = [0u8; 16];
            let closed = timeout(Duration::from_secs(8), socket.read(&mut bytes))
                .await
                .context("Stopped CONNECT remained open")?;
            ensure!(
                closed.as_ref().is_ok_and(|n| *n == 0)
                    || closed.as_ref().is_err_and(|e| matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::BrokenPipe
                    )),
                "Stopped CONNECT did not close"
            );
            active_is(&h.ssh, 0).await?;
            h.ssh.set_mode(Mode::Accept);
            h.close_client(client).await?;
            Ok(())
        }
    );
    for (label, content, code) in [
        ("empty", Some(Vec::new()), "PROXY_KEY_UNAVAILABLE"),
        (
            "oversized",
            Some(vec![0u8; 1024 * 1024 + 1]),
            "PROXY_KEY_TOO_LARGE",
        ),
        ("directory", None, "PROXY_KEY_UNAVAILABLE"),
        ("missing", None, "PROXY_KEY_UNAVAILABLE"),
    ] {
        case!(
            format!("{label} key rejected before SSH and posting API"),
            {
                let key = h.output.join(format!("{label}-key"));
                if let Some(bytes) = content {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&key)?
                        .write_all(&bytes)?;
                } else if label == "directory" {
                    fs::create_dir(&key)?;
                }
                let config = h
                    .settings(&format!("key-{label}"), json!({"private_key_file":key}))
                    .await?;
                let client = h.client_for(&config, &[]).await?;
                let before = h.ssh.attempts();
                let requests = h.requests.load(Ordering::SeqCst);
                h.expect_error(client, code).await?;
                let (_, post) = h
                    .call(
                        client,
                        "post_tweet",
                        json!({"account":"managed","text":"Invalid key must never transmit"}),
                    )
                    .await?;
                ensure!(
                    post["error"]["code"] == code,
                    "Posting did not reject {label} key"
                );
                ensure!(
                    h.ssh.attempts() == before && h.requests.load(Ordering::SeqCst) == requests,
                    "Invalid key reached SSH or posting API"
                );
                h.close_client(client).await?;
                Ok(())
            }
        );
    }
    for side in ["local", "remote"] {
        case!(
            format!("{side} port owner is preserved on bind conflict"),
            {
                let owner = TcpListener::bind(("127.0.0.1", 0)).await?;
                let port = owner.local_addr()?.port();
                let mut override_config = json!({});
                override_config[format!("{side}_port")] = json!(port);
                let config = h.settings(&format!("port-{side}"), override_config).await?;
                let client = h.client_for(&config, &[]).await?;
                h.expect_error(
                    client,
                    if side == "local" {
                        "PROXY_PORT_BUSY"
                    } else {
                        "PROXY_REMOTE_BIND_FAILED"
                    },
                )
                .await?;
                ensure!(owner.local_addr()?.port() == port, "Port owner was changed");
                let probe = TcpStream::connect(("127.0.0.1", port)).await?;
                drop(probe);
                h.success(client, "stop_proxy", json!({})).await?;
                h.close_client(client).await?;
                drop(owner);
                Ok(())
            }
        );
    }
    for (name, mode, overrides, code) in [
        (
            "host pin mismatch",
            Mode::Accept,
            json!({"host_key_sha256":format!("SHA256:{}",STANDARD_NO_PAD.encode([0u8;32]))}),
            "PROXY_HOST_KEY_MISMATCH",
        ),
        (
            "public key authentication rejected",
            Mode::AuthReject,
            json!({}),
            "PROXY_AUTH_FAILED",
        ),
        (
            "remote bind rejected",
            Mode::BindReject,
            json!({}),
            "PROXY_REMOTE_BIND_FAILED",
        ),
    ] {
        case!(name, {
            h.ssh.set_mode(mode);
            let config = h
                .settings(
                    &format!("failure-{:016x}", rand::random::<u64>()),
                    overrides,
                )
                .await?;
            let client = h.client_for(&config, &[]).await?;
            h.expect_error(client, code).await?;
            let status = h.success(client, "get_proxy_status", json!({})).await?;
            ensure!(
                state(&status) == "failed" && status["lastErrorCode"] == code,
                "Permanent SSH failure status changed"
            );
            h.success(client, "stop_proxy", json!({})).await?;
            h.close_client(client).await?;
            active_is(&h.ssh, 0).await?;
            Ok(())
        });
    }
    case!(
        "final MCP departure stops native daemon after idle period",
        {
            let config = h.settings("idle", json!({})).await?;
            let client = h.client_for(&config, &[]).await?;
            ensure!(
                state(
                    &h.success(client, "start_proxy", json!({"account":"managed"}))
                        .await?
                ) == "ready",
                "Idle test did not start proxy"
            );
            h.close_client(client).await?;
            active_is(&h.ssh, 0).await?;
            let probe = TcpListener::bind(("127.0.0.1", config.local)).await?;
            drop(probe);
            Ok(())
        }
    );
    case!("diagnostics and protocol never expose credentials", {
        h.diagnostics()?;
        Ok(())
    });
    let cleanup = h.close().await;
    suite.write_json("ssh-trace.json", &h.ssh.trace())?;
    suite.write_json("diagnostics.json", &h.diagnostics()?)?;
    suite.finish()?;
    cleanup?;
    Ok(())
}
