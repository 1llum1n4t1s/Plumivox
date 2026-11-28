# 拒否理由の分類

[api-reference.md](api-reference.md) の詳細。`src/api_rejection.rs`は提供元の拒否応答を、生のmessage・code・request IDを返さず固定名の分類（rejectionReasons）だけへ変換する。

## 分類名

`api_key`・`authentication`・`account_locked`・`account_suspended`・`rate_limit`・`credits`・`proxy`・`media`・`duplicate`・`upstream_timeout`・`upstream_unavailable`・`automation_suspected`・`daily_limit`・`text_too_long`・`session_mismatch`・`post_id_missing`。最後の5つは公知の文面に基づく候補で、提供元がその形で返す保証は未確認である。`post_id_missing`は投稿が作成された可能性があるため再送せず、利用者が照合する。

## Xのコードの読み取り

2xxで`status:"error"`（または`failed`・`success:false`）の本文からは、文面の部分一致に加え、最上位の`code`・`errors[].code`（整数か6桁以下の数字の文字列）と`x_status`を読み、次の表の分類を和集合で足す。

| コード | 分類 |
| --- | --- |
| 226 | `automation_suspected` |
| 344・185 | `daily_limit` |
| 187 | `duplicate` |
| 186 | `text_too_long` |
| 326 | `account_locked` |
| 64 | `account_suspended` |
| 88 | `rate_limit` |
| 32・215 | `authentication` |
| 89・353 | `session_mismatch` |
| 324 | `media` |

提供元が明記するのは226だけで、他は慣例に基づく候補である。コードか`x_status`由来の分類があれば`rejectionReasonSource`は`provider_code`。`x_status`が429・401で他に分類がなければ`rate_limit`・`authentication`とする。HTTPの非2xxの本文の整数は提供元自身のコードかもしれないので読まず、分類はHTTP状態だけから付ける。

## 応答に足す欄

表にあるコードの`xErrorCode`（最初の1つ）、100〜599の`x_status`の`xHttpStatus`、それらか表にない数字のコードがあるときの`xRejected: true`だけを足す。表にない生のコードは応答に出さず、拒否記録の`provider_codes`にだけ残す。`xRejected`は分類できないXの拒否も冷却ガードが扱えるようにするための印である。拒否文面を[記録](rejection-log.md)したときだけ`rejectionTextRecorded: true`を足し、文面自体は返さない。
