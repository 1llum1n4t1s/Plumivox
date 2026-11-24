# 팔로잉 피드의 가져오기 범위

TwitterAPI.io에는 X 공식 홈의 "팔로잉"을 직접 가져오는 API가 없습니다. Plumivox는 기본적으로 팔로우 목록의 앞에서부터 최대 100개 계정을 읽고, 20개 계정씩 묶어 공개 게시물을 검색합니다. `maxAccounts`는 20~200으로 설정할 수 있습니다. 첫 요청이 100이면 최대 6회, 200이면 최대 11회의 API 요청입니다. 최신 단가는 [TwitterAPI.io 요금](https://twitterapi.io/pricing)을 확인하세요.

결과의 `officialHomeEquivalent`는 항상 `false`입니다. 비공개 게시물은 가져올 수 없으며, 검색으로 얻는 내용·리포스트 처리·전체 정렬 순서가 공식 홈과 일치한다는 보장은 없습니다. 제공처에 다음 페이지가 있는 경우, 또는 고유한 비보호 검색 대상이 상한을 넘은 경우에는 `coverage.followingHasNextPage`가 `true`가 됩니다. `coverage.followingNextCursor`를 `followingCursor`에 전달하면 다음 팔로우 목록을 대상으로 합니다. 팔로우 목록은 최근 팔로우부터 정렬되며, 진행 중인 팔로우 변경으로 인한 가져오기 차이는 피할 수 없습니다.

`nextCursor`를 같은 계정의 `get_following_feed`의 `cursor`에 전달하면 각 검색 배치의 다음 내용을 가져옵니다. 페이지 안은 날짜·시간 순이지만, 배치별 페이지 경계 때문에 페이지를 넘나드는 전체 시간순은 보장하지 않습니다. 팔로우한 계정은 첫 목록으로 고정하며, 검색을 이어 갈 때 다시 읽지 않습니다. 피드의 cursor도 API 키로 서명하여 검색 대상의 변조를 거부합니다. 이전 버전의 서명 없는 cursor나 API 키 변경 전의 cursor는 사용할 수 없으므로, cursor를 빼고 다시 가져와 주세요. `cursor`와 `followingCursor`는 서로 다른 페이지 넘김이며 동시에 지정할 수 없습니다.

[팔로우 목록의 공식 응답 사양](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)에서는 보호 상태 필드를 확인할 수 없습니다. 같은 팔로우 목록 응답 안에서는 유효한 사용자 이름을 대소문자를 구분하지 않고 집계하며, 보호·공개·불명의 순서로 판정합니다. 보호로 판명된 대상을 제외한 첫 등장 순서로 상한까지 선택하고, 선택한 대상 중 판정할 수 없는 수를 `coverage.protectionStatusUnknownAccounts`로 반환합니다. `protectedAccountsExcluded`는 그 응답 안에서 보호로 판명된 고유 계정 수입니다. 중복·무효한 사용자 이름은 세지 않으며, 가져오기 대상 상한 밖의 보호 계정도 포함하므로 `maxAccounts`를 넘을 수 있습니다. 0이라고 해서 모든 대상이 공개 계정인 것은 아닙니다.

이전 구현이 발행한 서명된 cursor의 후속 요청은 검색 대상·건수뿐 아니라 미검색 범위·계속 여부도 이전 구현의 판정을 이어받으므로, 상한 밖의 대상을 건너뛸 수 있습니다. 새로운 보호 상태 집계·건수 계산·계속 판정으로 확실하게 다시 가져오려면 cursor를 빼 주세요.

게시물이나 팔로우 목록에 다음 내용이 있더라도, 제공처 cursor나 조합한 cursor가 길이 상한을 넘으면 계속할 수 없습니다. 이미 가져온 게시물과 `hasNextPage`는 유지하며, `nextCursor`는 `null`, `coverage.tweetContinuationAvailable`은 `false`, `tweetContinuationUnavailableReason`은 `cursor_too_long`이 됩니다. 팔로우 목록에서는 `followingNextCursor`와 `followingContinuationAvailable`·`followingContinuationUnavailableReason`을 확인하세요. cursor를 잘라내지 않으며, 이미 가져온 내용을 실패로 버리지 않습니다.

제공처가 한 번의 응답으로 `maxAccounts`를 넘는 고유한 비보호 대상을 반환한 경우, 상한 밖의 대상은 미검색 상태입니다. `followingHasNextPage`는 `true`, `followingContinuationUnavailableReason`은 `account_limit`이 되며, 팔로우 목록의 계속은 제공하지 않습니다(`followingContinuationAvailable: false`, `followingNextCursor: null`). 제공처의 다음 cursor로 미검색 대상을 건너뛰는 것을 막기 위해서입니다. `maxAccounts`가 200 미만이면 200까지의 범위에서 상한을 늘려 cursor 없이 다시 가져올 수 있습니다. 상한 200에서도 제공처가 과다 반환하는 경우에는 그 응답의 미검색 대상을 이 방법으로 가져올 수 없습니다. 같은 이름의 중복 행이나 보호·무효 이름만으로 상한을 넘은 경우에는 이 제한에 해당하지 않습니다. 선택된 대상의 게시물 후속 내용은 평소대로 가져올 수 있으며, 미검색 대상이 있는 상태도 이어받습니다.
