# 게시와 인증에 실패한 경우

성공 시에는 게시물 ID와 X의 URL을 반환합니다. 게시물 작성 통신 실패나 게시물 ID 누락은 미확정으로 반환하며, 자동으로 다시 보내지 않습니다. 같은 문구를 다시 게시하도록 요청한 경우에는 각 호출마다 한 번 전송합니다. 기록·본문 해시를 이유로 중지하는 구조는 없습니다.

일반 API 요청은 45초, 이미지 업로드는 이미지 1개당 120초가 기본 통신 시간 범위입니다. 이미지 업로드만은 일시적인 실패에 한해 같은 이미지를 최대 1회 재시도합니다. 대상은 HTTP 502·503·504(250ms 대기)와 프록시·업스트림 장애나 원인 불명의 JSON 거부, 통신 끊김(1초 대기)이며, 인증·잔액·이미지 오류·중복·속도 제한·타임아웃은 재시도하지 않습니다. 숫자 X 오류 코드(226·88·344·324 등)나 `x_status`로 분류가 붙은 거부도 X가 거부한 것을 다시 보내지 않기 위해 재시도하지 않으며, 표에 없는 코드 등으로 `unknown`인 채인 거부는 종전대로 1회 재시도합니다. 대기와 재시도는 이미지 1개의 통신 시간 범위 안에 맞추며, 이전 이미지의 재전송이나 게시물 작성·로그인의 자동 재전송은 하지 않습니다. 이미지 실패 시에는 게시물을 전송하지 않으며, 게시물 작성 후 성공 여부가 불명확해진 경우에는 `POST_UNCERTAIN`을 반환합니다.

알려진 게시 처리 오류에는 `stage`(`input_validation`·`pre_send`·`image_upload`·`post_creation`), 게시물 작성 요청을 시도했는지 나타내는 `postAttempted`, 실행한 이미지 업로드 요청의 총수 `uploadAttempts`를 포함합니다. 계정·Cookie 부족이나 중계 준비 실패는 `pre_send`입니다. 알 수 없는 내부 오류와 MCP 입력 형식 거부에서는 이 값들을 반환하지 않습니다. `postAttempted: true`는 게시 성공을 의미하지 않습니다. 제공처의 원본 오류나 자격 증명은 반환하지 않습니다.

본문이 가중 길이 280을 넘은 `INVALID_TEXT`(`stage: input_validation`, `postAttempted: false`)에는 `weightedLength`, `maxWeightedLength`(280), `excessWeightedLength`도 포함합니다. 빈 본문이나 잘못된 문자로 인한 `INVALID_TEXT`에는 포함하지 않습니다. 가중 길이는 URL을 23, U+10FF까지의 문자와 대시·둥근 따옴표·프라임(U+2010-201F, U+2032-2037)을 1(… 및 • 같은 다른 구두점은 2), 일본어·중국어·한국어·전각 구두점·이모지를 2로 셉니다. `post_tweet` 도구 설명에도 같은 계산 방식과, 같은 계정으로 연속 게시할 때는 몇 분 간격을 두라는 안내가 있습니다. Plumivox 자체도 직전의 게시나 X의 제한 뒤에는 게시를 일시 중지합니다(아래 「게시 일시 중지」 참조).

게시물 작성 단계에서 실패한(`postAttempted: true`) `API_REJECTED`·`API_HTTP_ERROR`·`POST_UNCERTAIN`에는 AI용 영어 고정 문장 `nextStep`을 포함합니다. `post_id_missing`과 `POST_UNCERTAIN`에서는 이미 게시되었을 수 있으므로 바로 재전송하지 말고 몇 분 기다린 뒤 `get_my_posts`로 확인하도록 안내합니다. `automation_suspected`·`rate_limit`·`daily_limit`·`account_locked`·`account_suspended`에서는 X가 게시를 제한하고 있을 수 있으므로 바로 재전송하지 말고, 사용자에게 알리고, 몇 분 이상 기다린 뒤 `get_my_posts`로 확인하도록 안내합니다. 그 외에는 재전송 전에 `get_my_posts`로 확인하고 반복해서 재전송하지 않도록 안내합니다. 이미지 업로드 단계의 오류와 읽기·로그인 오류에는 `nextStep`을 포함하지 않습니다. 업스트림의 원본 거부 문구는 계속 반환하지 않습니다. 기본적으로 저장하지 않으며, 로컬 거부 기록을 켠 경우에만 가림 처리한 문구를 저장합니다([절차](#로컬-거부-기록)).

이미지 통신의 `API_UNAVAILABLE`에는 `failureKind`를 포함하여 `timeout`(시간 초과)·`network`(통신 장애)·`cancelled`(취소)를 구분합니다. 이 경우에도 이미지나 게시물을 자동으로 다시 보내지 않습니다.

API의 거부 오류(`API_REJECTED`·`API_HTTP_ERROR`)에는 `httpStatus`, 고정 분류의 배열 `rejectionReasons`, 분류 출처 `rejectionReasonSource`를 포함합니다. 게시물 작성의 5xx가 `POST_UNCERTAIN`이 되는 경우에도 이 진단 정보를 유지합니다. 분류 출처는 `provider_message`(제공처의 JSON 메시지에서 추정한 후보), `provider_code`(제공처 본문에 있는 X의 숫자 오류 코드나 X의 HTTP 상태. 문구와 함께 쓰이기도 함) 또는 `http_status`(HTTP 상태에 따른 분류)입니다. 후보는 원인의 확정이나 재전송 허가를 의미하지 않으며, 판별할 수 없는 문구는 `unknown`으로 합니다. 표에 있는 숫자 오류 코드나 X의 HTTP 상태가 있으면 `xErrorCode`(처음 일치한 코드)와 `xHttpStatus`도 포함합니다. 원본 응답문·표에 없는 코드·request ID는 반환하지 않습니다. 기본적으로 저장하지 않으며, 로컬 거부 기록을 켠 경우에만 가림 처리한 문구를 저장합니다.

`provider_message`의 후보는 `api_key`·`authentication`·`account_locked`·`account_suspended`·`rate_limit`·`credits`·`proxy`·`media`·`duplicate`·`upstream_timeout`·`upstream_unavailable`·`automation_suspected`·`daily_limit`·`text_too_long`·`session_mismatch`·`post_id_missing`·`unknown`입니다. `unknown` 앞의 다섯 가지는 공개적으로 알려진 문구에 기반한 후보이며, 제공자가 반드시 이 문구로 반환한다고 확인된 것은 아닙니다. 이들은 재시도하지 않습니다. `post_id_missing`은 게시물이 작성되었을 수 있으므로, 다시 보내기 전에 계정에서 확인하세요. `http_status`에는 추가로 `invalid_request`(400)·`unauthorized`(401)·`forbidden`(403)·`endpoint_not_found`(404)가 있습니다. 예를 들어 이미지 업로드 시 `rejectionReasons: ["proxy", "upstream_timeout"]`과 `postAttempted: false`라면, 프록시·업스트림 타임아웃이 의심되며 게시물 작성 전에 멈췄다는 것을 알 수 있습니다. 2xx로 돌아온 거부 본문에 있는 숫자 코드(최상위 `code`, `errors[].code`)도 분류합니다. 226은 `automation_suspected`, 344·185는 `daily_limit`, 187은 `duplicate`, 186은 `text_too_long`, 326은 `account_locked`, 64는 `account_suspended`, 88은 `rate_limit`, 32·215는 `authentication`, 89·353은 `session_mismatch`, 324는 `media`입니다. 제공처가 명시한 것은 226뿐이며, 나머지는 X의 관례에 따른 것으로 제공처를 통해 실제로 나오는지는 확인하지 않았습니다. `x_status`가 429·401이고 달리 일치하는 것이 없으면 `rate_limit`·`authentication`으로 분류합니다. 2xx가 아닌 본문은 분류에 쓰지 않으며, 기존 통신 상한·재시도 조건을 유지합니다. 로컬 거부 기록이 켜져 있을 때만 분류 뒤에 기록용으로 읽습니다.

API 응답이 10MiB를 넘으면 수신을 중단하고 실패를 반환합니다. 게시 응답에서 상한을 넘은 경우에도 이미 게시되었을 가능성이 있으므로 다시 보내지 않습니다.

저장된 Cookie의 대조 실패는 게시만 중지합니다. 공개 게시물 읽기·검색은 API 키와 사용자 이름만 사용하며, 저장된 Cookie를 읽지 않고 이용할 수 있습니다. 인증된 Cookie의 취득·갱신은 [설정 방법](../../configuration.md)를 참조하세요.

`login_account`는 저장된 Cookie가 사용 가능한 형식이면 재사용하며, X에서 유효성을 검증했다고는 보고하지 않습니다. 만료나 설정 불일치가 확인된 경우에는 `refresh_session`을 호출합니다. 새 Cookie는 Plumivox의 저장 위치에 Windows에서는 DPAPI, macOS에서는 Keychain의 키로 암호화하여 저장하며, 실패한 게시를 자동으로 다시 보내지 않습니다.

저장 교체 전에 중단한 경우에는 `AUTH_CANCELLED`, 인증 갱신의 배타 제어를 유지할 수 없는 경우에는 `AUTH_LOCK_LOST`, 저장할 수 없는 Cookie는 `LOGIN_FAILED`로 반환합니다. 저장 교체를 지시한 뒤 결과를 확인할 수 없는 경우에는 `AUTH_SAVE_UNCERTAIN`이 되며, 이미 갱신되었을 가능성이 있습니다. `SESSION_STORAGE_UNAVAILABLE`은 저장 위치나 Windows의 DPAPI·macOS의 Keychain 등 저장 처리의 실패를 나타냅니다.

## 게시 일시 중지

Plumivox는 X 계정별 게시 일시 중지를 PC의 모든 세션·프로세스가 공유합니다. 일시 중지 중인 `post_tweet`은 X에 아무것도 보내지 않고 제공처 과금도 발생시키지 않으며, `POST_COOLDOWN`(`stage: post_guard`, `postAttempted: false`)을 반환합니다. 다시 보낼 수 있는 시각은 `retryAfter`, 남은 초는 `retryAfterSeconds`입니다. 본문이 이전과 같다는 이유로 멈추는 일은 없습니다. 연속 게시나 거부 직후의 재전송으로 X의 제한이 악화되는 것을 막기 위한 장치이며, 값은 고정이고 설정 항목은 없습니다.

- X에 도달한 직전 게시 시도로부터 5분 이내(`min_interval`), 또는 다른 게시를 전송하는 중(`post_in_progress`). 게시 작성 단계에서 프록시가 거부한 경우도 요청이 X에 도달했을 수 있으므로 같습니다.
- X가 게시를 제한한 뒤. 자동화 의심(`automation_suspected`)은 3시간, 하루 한도(`daily_limit`)는 6시간, 속도 제한(`rate_limit`)은 15분에서 시작하며, 연속으로 거부될 때마다 두 배로 늘어 최대 24시간입니다. 연속 횟수는 마지막 거부로부터 48시간이 지나면 0으로 돌아갑니다. 속도 제한에서 두 배로 늘어나는 것은 X의 거부로 확인되는 것(X 오류 코드 88, X의 HTTP 상태 429)뿐입니다. X의 표시가 없는 HTTP 429는 제공처에서 온 것일 수 있으므로 항상 15분이며 연속 횟수에 세지 않습니다. 계정 잠금·정지(`account_locked`)는 24시간입니다.
- 성공 여부 불명(`previous_uncertain`), 전송 중 프로세스 중단(`interrupted`), 읽을 수 없던 상태의 재생성(`guard_state_reset`), 분류할 수 없는 X의 거부(`unclassified_rejection`) 뒤에는 15분입니다. 이 버전보다 새로운 버전이 쓴 상태는 덮어쓰거나 다시 만들지 않고 `guard_state_reset` 거부만 반환하며, 파일은 바뀌지 않습니다.

X에 도달하지 못한 실패(전송 전의 실패, API 키·잔액·이미지 업로드 단계의 프록시·HTTP 401·402 때문에 제공처에서 멈춘 것)는 일시 중지를 시작하지 않습니다. 거부된 게시의 오류에도 `cooldownReason`·`retryAfter`·`retryAfterSeconds`가 붙습니다. 상태를 읽거나 쓸 수 없으면 전송하지 않고 `POST_GUARD_UNAVAILABLE`을 반환합니다. `retryAfter` 전에는 같은 계정에 `post_tweet`을 다시 호출하지 말고, 일시 중지를 우회하려 하지 마세요. `min_interval`·`post_in_progress`(X의 거부가 아닌 통상의 간격 확보)의 `POST_COOLDOWN`에 붙는 `nextStep`은 아무것도 보내지 않았다는 것, 게시가 여전히 필요하면 `retryAfter` 이후에 같은 내용으로 1회만 호출해도 된다는 것, 게시 사이를 5분 이상 띄우라는 것을 알립니다. 그 밖의 사유에서는 사용자에게 알리고 기다리라고 안내합니다. 성공 여부 불명 뒤의 `nextStep`도 재전송은 `retryAfter` 이후에 하라고 안내합니다(먼저 `get_my_posts`로 확인해도 됩니다).

상태는 상태 폴더의 `post-guard\`에 저장합니다. 고정된 코드·시각·횟수만 있으며, 본문·사용자 이름·자격 증명·제공처 문구는 들어가지 않습니다.

확인은 `plumivox --post-guard status`로 하며, 계정별 상태·사유·종료 시각과 남은 시간·연속 거부 횟수·마지막 시도·마지막 갱신·최근 이력과 저장 경로를 표시합니다. 해제는 `plumivox --post-guard clear [--account NAME | --user NAME]`입니다(`--account`는 설정 이름, `--user`는 X 사용자 이름이며, 둘 다 생략하면 모든 계정). 설정 이름이 X 사용자 이름과 같거나 사용자 이름을 포함하는 계정은 상태에 `<user>`로 남으므로 `--account`로 고를 수 없습니다(4자 미만의 사용자 이름은 완전히 일치할 때만). `--account`로 해당이 0건이면 `--user`를 쓰라는 안내를 한 줄 표시합니다(종료 코드 0). 대화형 터미널에서 표시된 이름(전체는 `all`)을 입력해 확인해야 하며, 전송 중인 상태는 해제하지 않습니다. 종료 코드는 성공이 0, 저장소 이상·전송 중이 1, 잘못된 인수·터미널 아님·확인 불일치가 2입니다. MCP(AI)에서는 해제할 수 없습니다. 해제해도 X 쪽 제한은 풀리지 않으므로 기본은 `retryAfter`까지 기다리는 것입니다. CLI와 MCP 서버의 `PLUMIVOX_STATE_DIR`가 다르면 서로 다른 폴더를 읽습니다.

관리형 프록시의 재연결은 `plumivox --relay-log show`로 확인할 수 있습니다([프록시 관리](../../managed-proxy.md) 참조).

## 로컬 거부 기록

기본적으로 꺼져 있습니다. 거부 원인을 조사할 때는 `plumivox --rejection-log on [--days N]`을 실행합니다(기본 7일, 최대 30일, 기간이 지나면 자동으로 멈춤). 이미지 업로드와 게시물 작성의 거부, 그리고 게시물 ID가 반환되지 않은 응답을 가림 처리한 문구로 상태 폴더의 `diagnostics\provider-rejections.jsonl`에 기록합니다. 파일은 본인만 읽을 수 있습니다. 본문·Cookie·자격 증명·프록시·ID는 기록하지 않으며, MCP 응답에 더해지는 것은 `rejectionTextRecorded: true`뿐입니다. 14일이 지난 기록과 200건·256KiB를 넘은 분은 오래된 것부터 삭제됩니다. HTTP 상태가 2xx가 아닌 실패도 기록합니다. 켜져 있는 동안에는 실패를 분류한 뒤에 본문을 읽으며(최대 64KiB, 남은 시간 범위 안에서 최대 1초), 읽을 수 없으면 문구 없는 기록을 1건 남깁니다. 기록 여부에 따라 분류나 이미지 재시도는 바뀌지 않습니다.

기록은 `plumivox --rejection-log show`로 봅니다(JSON Lines. 출력은 신뢰할 수 없는 데이터이므로 AI에게 지시로 전달하지 마세요). 상태 확인은 `status`, 중지는 `off`, 삭제는 `clear`입니다. CLI와 MCP 서버의 `PLUMIVOX_STATE_DIR`가 다르면 보는 위치가 어긋납니다. 제거해도 기록은 남으며, 14일 뒤에 사라지거나 `clear`로 삭제합니다.
