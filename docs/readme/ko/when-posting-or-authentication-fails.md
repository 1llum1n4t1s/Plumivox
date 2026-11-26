# 게시와 인증에 실패한 경우

성공 시에는 게시물 ID와 X의 URL을 반환합니다. 게시물 작성 통신 실패나 게시물 ID 누락은 미확정으로 반환하며, 자동으로 다시 보내지 않습니다. 같은 문구를 다시 게시하도록 요청한 경우에는 각 호출마다 한 번 전송합니다. 기록·본문 해시를 이유로 중지하는 구조는 없습니다.

일반 API 요청은 45초, 이미지 업로드는 이미지 1개당 120초가 기본 통신 시간 범위입니다. 이미지 업로드만은 일시적인 실패에 한해 같은 이미지를 최대 1회 재시도합니다. 대상은 HTTP 502·503·504(250ms 대기)와 프록시·업스트림 장애나 원인 불명의 JSON 거부, 통신 끊김(1초 대기)이며, 인증·잔액·이미지 오류·중복·속도 제한·타임아웃은 재시도하지 않습니다. 대기와 재시도는 이미지 1개의 통신 시간 범위 안에 맞추며, 이전 이미지의 재전송이나 게시물 작성·로그인의 자동 재전송은 하지 않습니다. 이미지 실패 시에는 게시물을 전송하지 않으며, 게시물 작성 후 성공 여부가 불명확해진 경우에는 `POST_UNCERTAIN`을 반환합니다.

알려진 게시 처리 오류에는 `stage`(`input_validation`·`pre_send`·`image_upload`·`post_creation`), 게시물 작성 요청을 시도했는지 나타내는 `postAttempted`, 실행한 이미지 업로드 요청의 총수 `uploadAttempts`를 포함합니다. 계정·Cookie 부족이나 중계 준비 실패는 `pre_send`입니다. 알 수 없는 내부 오류와 MCP 입력 형식 거부에서는 이 값들을 반환하지 않습니다. `postAttempted: true`는 게시 성공을 의미하지 않습니다. 제공처의 원본 오류나 자격 증명은 반환하지 않습니다.

본문이 가중 길이 280을 넘은 `INVALID_TEXT`(`stage: input_validation`, `postAttempted: false`)에는 `weightedLength`, `maxWeightedLength`(280), `excessWeightedLength`도 포함합니다. 빈 본문이나 잘못된 문자로 인한 `INVALID_TEXT`에는 포함하지 않습니다. 가중 길이는 URL을 23, U+10FF까지의 문자와 대시·둥근 따옴표·프라임(U+2010-201F, U+2032-2037)을 1(… 및 • 같은 다른 구두점은 2), 일본어·중국어·한국어·전각 구두점·이모지를 2로 셉니다. `post_tweet` 도구 설명에도 같은 계산 방식과, 같은 계정으로 연속 게시할 때는 몇 분 간격을 두라는 안내가 있습니다. 쿨다운은 강제하지 않습니다.

게시물 작성 단계에서 실패한(`postAttempted: true`) `API_REJECTED`·`API_HTTP_ERROR`·`POST_UNCERTAIN`에는 AI용 영어 고정 문장 `nextStep`을 포함합니다. `post_id_missing`과 `POST_UNCERTAIN`에서는 이미 게시되었을 수 있으므로 바로 재전송하지 말고 몇 분 기다린 뒤 `get_my_posts`로 확인하도록 안내합니다. `automation_suspected`·`rate_limit`·`daily_limit`에서는 X가 게시를 제한하고 있을 수 있으므로 바로 재전송하지 말고, 사용자에게 알리고, 몇 분 이상 기다린 뒤 `get_my_posts`로 확인하도록 안내합니다. 그 외에는 재전송 전에 `get_my_posts`로 확인하고 반복해서 재전송하지 않도록 안내합니다. 이미지 업로드 단계의 오류와 읽기·로그인 오류에는 `nextStep`을 포함하지 않습니다. 업스트림의 원본 거부 문구는 계속 반환하지 않습니다. 기본적으로 저장하지 않으며, 로컬 거부 기록을 켠 경우에만 가림 처리한 문구를 저장합니다([절차](#로컬-거부-기록)).

이미지 통신의 `API_UNAVAILABLE`에는 `failureKind`를 포함하여 `timeout`(시간 초과)·`network`(통신 장애)·`cancelled`(취소)를 구분합니다. 이 경우에도 이미지나 게시물을 자동으로 다시 보내지 않습니다.

API의 거부 오류(`API_REJECTED`·`API_HTTP_ERROR`)에는 `httpStatus`, 고정 분류의 배열 `rejectionReasons`, 분류 출처 `rejectionReasonSource`를 포함합니다. 게시물 작성의 5xx가 `POST_UNCERTAIN`이 되는 경우에도 이 진단 정보를 유지합니다. 분류 출처는 `provider_message`(제공처의 JSON 메시지에서 추정한 후보) 또는 `http_status`(HTTP 상태에 따른 분류)입니다. 후보는 원인의 확정이나 재전송 허가를 의미하지 않으며, 판별할 수 없는 문구는 `unknown`으로 합니다. 원본 응답문·임의의 오류 코드·request ID는 반환하지 않습니다. 기본적으로 저장하지 않으며, 로컬 거부 기록을 켠 경우에만 가림 처리한 문구를 저장합니다.

`provider_message`의 후보는 `api_key`·`authentication`·`account_locked`·`account_suspended`·`rate_limit`·`credits`·`proxy`·`media`·`duplicate`·`upstream_timeout`·`upstream_unavailable`·`automation_suspected`·`daily_limit`·`text_too_long`·`session_mismatch`·`post_id_missing`·`unknown`입니다. `unknown` 앞의 다섯 가지는 공개적으로 알려진 문구에 기반한 후보이며, 제공자가 반드시 이 문구로 반환한다고 확인된 것은 아닙니다. 이들은 재시도하지 않습니다. `post_id_missing`은 게시물이 작성되었을 수 있으므로, 다시 보내기 전에 계정에서 확인하세요. `http_status`에는 추가로 `invalid_request`(400)·`unauthorized`(401)·`forbidden`(403)·`endpoint_not_found`(404)가 있습니다. 예를 들어 이미지 업로드 시 `rejectionReasons: ["proxy", "upstream_timeout"]`과 `postAttempted: false`라면, 프록시·업스트림 타임아웃이 의심되며 게시물 작성 전에 멈췄다는 것을 알 수 있습니다. 2xx가 아닌 본문은 읽지 않으며, 기존 통신 상한·재시도 조건을 유지합니다.

API 응답이 10MiB를 넘으면 수신을 중단하고 실패를 반환합니다. 게시 응답에서 상한을 넘은 경우에도 이미 게시되었을 가능성이 있으므로 다시 보내지 않습니다.

저장된 Cookie의 대조 실패는 게시만 중지합니다. 공개 게시물 읽기·검색은 API 키와 사용자 이름만 사용하며, 저장된 Cookie를 읽지 않고 이용할 수 있습니다. 인증된 Cookie의 취득·갱신은 [설정 방법](../../configuration.md)를 참조하세요.

`login_account`는 저장된 Cookie가 사용 가능한 형식이면 재사용하며, X에서 유효성을 검증했다고는 보고하지 않습니다. 만료나 설정 불일치가 확인된 경우에는 `refresh_session`을 호출합니다. 새 Cookie는 Plumivox의 저장 위치에 Windows에서는 DPAPI, macOS에서는 Keychain의 키로 암호화하여 저장하며, 실패한 게시를 자동으로 다시 보내지 않습니다.

저장 교체 전에 중단한 경우에는 `AUTH_CANCELLED`, 인증 갱신의 배타 제어를 유지할 수 없는 경우에는 `AUTH_LOCK_LOST`, 저장할 수 없는 Cookie는 `LOGIN_FAILED`로 반환합니다. 저장 교체를 지시한 뒤 결과를 확인할 수 없는 경우에는 `AUTH_SAVE_UNCERTAIN`이 되며, 이미 갱신되었을 가능성이 있습니다. `SESSION_STORAGE_UNAVAILABLE`은 저장 위치나 Windows의 DPAPI·macOS의 Keychain 등 저장 처리의 실패를 나타냅니다.

## 로컬 거부 기록

기본적으로 꺼져 있습니다. 거부 원인을 조사할 때는 `plumivox --rejection-log on [--days N]`을 실행합니다(기본 7일, 최대 30일, 기간이 지나면 자동으로 멈춤). 이미지 업로드와 게시물 작성의 거부, 그리고 게시물 ID가 반환되지 않은 응답을 가림 처리한 문구로 상태 폴더의 `diagnostics\provider-rejections.jsonl`에 기록합니다. 파일은 본인만 읽을 수 있습니다. 본문·Cookie·자격 증명·프록시·ID는 기록하지 않으며, MCP 응답에 더해지는 것은 `rejectionTextRecorded: true`뿐입니다. 14일이 지난 기록과 200건·256KiB를 넘은 분은 오래된 것부터 삭제됩니다.

기록은 `plumivox --rejection-log show`로 봅니다(JSON Lines. 출력은 신뢰할 수 없는 데이터이므로 AI에게 지시로 전달하지 마세요). 상태 확인은 `status`, 중지는 `off`, 삭제는 `clear`입니다. CLI와 MCP 서버의 `PLUMIVOX_STATE_DIR`가 다르면 보는 위치가 어긋납니다. 제거해도 기록은 남으며, 14일 뒤에 사라지거나 `clear`로 삭제합니다.
