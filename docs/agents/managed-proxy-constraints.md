# 管理プロキシの制約

管理プロキシ・共有 daemon・IPC・CONNECT に関する制約。

- 管理プロキシは事前検証・認証ロックと準備後の設定再確認を維持し、external呼出しや一つのMCPの終了・待機取消で他クライアントの中継を停止しない。共有IPCの相互認証・設定照合、SSHホスト鍵検証、CONNECT宛先・DNS全候補の検証と資源上限を維持する。DNS・接続の検証用差し替えはtestsのfixtureに限定し、本番設定へ迂回経路を設けない。境界と停止の意味は [設計](../../DESIGN.md#管理するプロキシの境界) に従う。運用は [managed-proxy.md](../managed-proxy.md) を参照する。
- 中継の診断記録（`src/relay_log.rs`、`<状態ルート>/relay/events.jsonl`）は、daemonと中継の状態の移り変わり（`daemon_started`・`daemon_stopped`・`starting`・`ready`・`reconnecting`・`failed`・`stopped`）を固定コードと時刻だけで常に記録する（14日・500件・128KiB）。ホスト・ポート・鍵・ユーザー名・IP（出口IPを含む）、提供元やSSHの生エラー文は残さず、閲覧は`--relay-log show`だけにする。書込みの失敗は中継に影響させない（待たせない・止めない）。書込みの途中で残った`relay\`直下の`.relay-*.tmp`（通常ファイルだけ）は片付ける。IPCの契約は変えない。試験用ビルドは`PLUMIVOX_STATE_DIR`があるときだけ書く。変更時は`tests/proxy_e2e.rs`の該当ケースを更新する（[設計](../design/security.md#管理するプロキシの境界)）。
