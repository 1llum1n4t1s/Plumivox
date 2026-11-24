# 追蹤中動態的取得範圍

TwitterAPI.io 沒有直接取得 X 官方首頁「追蹤中」的 API。Plumivox 預設會從追蹤清單開頭讀取最多 100 個帳號，並每 20 個帳號為一組搜尋公開貼文。`maxAccounts` 可設定為 20～200。首次為 100 時最多 6 次，為 200 時最多 11 次 API 要求。最新單價請確認 [TwitterAPI.io 的費用](https://twitterapi.io/pricing)。

結果中的 `officialHomeEquivalent` 一律為 `false`。無法取得受保護的貼文，且透過搜尋取得的內容、轉發的處理方式與整體排列順序，都不保證與官方首頁一致。當提供者還有後續資料，或唯一且未受保護的搜尋對象超過上限時，`coverage.followingHasNextPage` 會是 `true`。將 `coverage.followingNextCursor` 傳給 `followingCursor`，即可以下一段追蹤對象清單為對象。追蹤清單從最新的追蹤開始排列，無法避免因持續的追蹤變更而產生的取得差異。

將 `nextCursor` 傳給同一帳號 `get_following_feed` 的 `cursor`，即可取得各搜尋批次的後續內容。頁面內依時間排序，但由於各批次的頁面邊界，不保證跨頁的整體時間順序。追蹤對象會固定為首次取得的清單，在搜尋的後續中不會重新讀取。動態的 cursor 也會以 API 金鑰簽署，並拒絕遭竄改的搜尋對象。舊版未簽署的 cursor，以及變更 API 金鑰前的 cursor 無法使用，請移除 cursor 後重新取得。`cursor` 與 `followingCursor` 是不同的分頁方式，無法同時指定。

在[追蹤清單的官方回應規格](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)中，無法確認保護狀態的欄位。在同一份追蹤清單的回應中，會將有效的使用者名稱不區分大小寫地彙整，並依受保護、公開、不明的順序進行判定。排除已確認受保護的對象後，依首次出現的順序選取至上限，並將所選對象中無法判定的數量回傳於 `coverage.protectionStatusUnknownAccounts`。`protectedAccountsExcluded` 是該回應中已確認受保護的唯一帳號數。重複或無效的使用者名稱不計入，且會包含取得對象上限以外的受保護帳號，因此可能超過 `maxAccounts`。即使為 0，也不代表所有對象都是公開帳號。

舊實作所發出之已簽署 cursor 的後續，除了搜尋對象與件數外，也會沿用舊實作對未搜尋範圍與可否繼續的判定，因此可能會略過上限以外的對象。若要以新的保護狀態彙整、件數計算與繼續判定確實重新取得，請移除 cursor。

即使貼文或追蹤清單還有後續，若提供者的 cursor 或組合後的 cursor 超過長度上限，也無法繼續。已取得的貼文與 `hasNextPage` 會保留，`nextCursor` 為 `null`，`coverage.tweetContinuationAvailable` 為 `false`，`tweetContinuationUnavailableReason` 為 `cursor_too_long`。追蹤清單請確認 `followingNextCursor` 與 `followingContinuationAvailable`、`followingContinuationUnavailableReason`。cursor 不會被截斷，已取得的內容也不會被視為失敗而捨棄。

若提供者在一次回應中回傳超過 `maxAccounts` 的唯一未受保護對象，上限以外的對象就是未搜尋的。`followingHasNextPage` 為 `true`，`followingContinuationUnavailableReason` 為 `account_limit`，且不提供追蹤清單的繼續（`followingContinuationAvailable: false`、`followingNextCursor: null`）。這是為了防止透過提供者的下一個 cursor 略過未搜尋的對象。若 `maxAccounts` 小於 200，可在 200 以內的範圍提高上限，並在不帶 cursor 的情況下重新取得。若上限為 200 時提供者仍過量回傳，則無法以此方法取得該回應中未搜尋的對象。即使僅因同名重複列或受保護、無效名稱而超過上限，也不適用此限制。已選取對象的貼文後續可照常取得，且未搜尋對象存在的狀態也會沿用。
