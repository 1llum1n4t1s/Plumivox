# VPS インストーラーの制約

`setup/vps/` と VPS インストーラー（Rust 版・Bash 互換経路）に関する制約。初期構築の境界は [設計](../../DESIGN.md#初期設定と公開中継の構築) を参照する。

- `setup/vps/` は `.gitattributes` に従いLFで保存する。VPSインストーラーの事前確認と管理対象の照合、専用SSHユーザーの制限、認証付きX宛先限定CONNECT、住宅回線出口不在時の直接接続禁止を維持する。
- VPSはx86_64・aarch64の静的Rustインストーラーを優先し、他CPU用のBash互換経路を維持する。新規導入時にACL・拡張属性・SELinux・必要なcapabilityを安全に扱えなければ変更前に拒否し、既存の同一導入の読取り確認と区別する。SSH設定交換時のroot限定バックアップディレクトリを自動清掃せず、同時管理操作を避け、旧ファイル記述子の利用終了・新旧内容・SSH管理接続の動作を本人が確認してから整理する。`RENAME_EXCHANGE`非対応時の安全な失敗を許容し、Bash版で安全条件を迂回しない。
- Bash版の新規導入にも`mv --exchange --no-copy -T`と`ln -T`対応GNU coreutils、`getfacl`・`getfattr`・`sync`・`readlink`・`lsattr`、初期user namespace内の`CAP_SYS_ADMIN`と属性の安全確認・同一ファイルシステム内の交換を要求する。未対応・未導入なら変更前に停止し、本人の環境確認前に自動導入・更新しない。Bash版も旧inodeをroot限定バックアップへ保持し、本人確認後に整理する。`--check`は書込みprobeをしないため、成功後でも導入時に停止し得る。
- VPS新規導入時はSSH設定と親のmount ID、inode flagsも確認し、別mount・確認不能・既存名との競合を拒否する。清掃失敗時はstageとバックアップの通知された保持先を確認し、今回分と交換状態を本人が確認するまで一括削除・再実行しない。
