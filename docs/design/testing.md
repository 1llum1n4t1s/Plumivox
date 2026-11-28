# 検証の構成と設計判断

[DESIGN.md](../../DESIGN.md) の検証に関する詳細。失敗条件とE2Eの方針は [docs/verification.md](../verification.md)、コマンドは [AGENTS.md](../../AGENTS.md) を参照する。

## テスト部品

| テスト | 検証対象 |
| --- | --- |
| `tests/api_e2e.rs` / `tests/native_e2e.rs` / `tests/tls_e2e.rs` / `tests/support/` / `tests/native-fixture.rs` | Rustの偽HTTP・HTTPS APIと別プロセスのMCPを使い、通信内容・回数・失敗状態を検証する。失敗注入は`test-fixture`ビルドに限定する |
| `tests/proxy_e2e.rs`と`src/proxy.rs` / `src/proxy_egress.rs` / `src/proxy_ipc.rs`内のテスト | Rustの偽SSH・echo、認証付きIPCとCONNECT出口の境界、中継の診断記録（`--relay-log show`、固定コードだけが残ること）を検証する |
| `tests/setup_e2e.rs` / `tests/setup_vps_e2e.rs` | Windows・macOSの設定ウィザード、個別保存先、鍵の保護、VPS CLIの検証と拒否条件。実VPSへの導入は別途確認が必要 |
| `tests/setup_velopack_e2e.rs` / `tests/cli-stub.rs` | Setup（Velopack）の隔離E2E（[Setup](setup-velopack.md)） |

## 冷却ガードと診断記録の検証

- **冷却ガード**（`tests/api_post_cases.rs`の`guard_cases`）：9ケース。複数のMCPプロセスで冷却を共有して期限で終わること、別プロセスの送信中は`post_in_progress`で止まること、送信中にプロセスを終了すると`interrupted`の冷却が残ること、拒否で冷却が始まり次の投稿は送られないこと、状態ファイルとCLIの`status`に秘密・提供元の文面が出ないこと、`xRejected`の無いHTTP 429は固定15分でstrikeを数えず（対照のコード88はstrike 1）、投稿作成段階の`proxy`だけの拒否は最小間隔の5分で画像段階の`proxy`拒否は冷却なしであること、設定名がユーザー名を含むだけでも状態とCLIの`status`に名前が出ず`<user>`になること、新しいschemaの状態は投稿を断り（`guard_state_reset`）ファイルを変えないこと。冷却中は偽APIへの要求数が増えないことも確かめる。
- **数字コードの分類**：偽APIの場面（名前は`rejection-`に続く。例`rejection-code226`）`code226`・`code344`・`code344s`（文字列）・`code88`・`codeunknown`（表にないコードは応答に出さない）・`xstatus`・`http400`（HTTP 400の`{error:226}`をXのコードとして扱わない）・`http429`で、`xErrorCode`・`xHttpStatus`・`xRejected`と分類を確かめる。数字のコードや`x_status`で分類が付いた画像アップロードの拒否は再試行されず（226・`x_status`は1要求）、`unknown`のままの拒否は従来どおり1回再試行され2要求になることも固定する（`Image upload is not retried when a numeric code classifies the refusal`）。保留用の場面`upload-hold`（画像アップロードの応答を解除まで保留する）は送信中の検証に使う。
- **拒否記録の追加ケース**：偽APIの場面`rejection-http400detail`（`detail`欄に本文・Cookie・ユーザー名・プロキシ・鍵を混ぜたHTTP 400で、伏字になり本文が残らないこと）、`rejection-http400big`（64KiBを超えるHTTP 400は文面なしの1件になること）、`create502-stream`（本文が終わらずにゆっくり届く投稿作成の502で、本文の読み込みが1秒の上限で打ち切られ、画像の再試行が記録の有無で変わらないこと）。
- **中継の診断記録**：`tests/proxy_e2e.rs`が、再接続を挟む`ready`の繰り返しと`daemon_stopped`、`--relay-log show`の出力と、記録が無いときにフォルダーを作らないことを確かめる。
- **試験用の切替**：冷却ガードの時間は試験用ビルド（`test-fixture`）だけの環境変数`PLUMIVOX_POST_GUARD_TEST`で縮める（未設定・`off`は無効、整数は「1分の代わりにするms」）。本番ビルドは環境変数を読まず常に有効。中継の診断記録は、試験用ビルドでは`PLUMIVOX_STATE_DIR`があるときだけ書く（未設定のまま本物の保存先へ書かないため）。

## 設計判断

- **配布と検証**：ネイティブ実行ファイルを同梱し、利用者側のNode.js・Rust・`node_modules`を不要にする。Rustソース変更時は対象OSの配布物を再ビルドする。E2EとRustテストは実際のXへの副作用を避けつつMCP・HTTP・OS別の保存とプロキシ境界を検証するが、実サービス上のCookie有効性や投稿成功を証明するものではない。
