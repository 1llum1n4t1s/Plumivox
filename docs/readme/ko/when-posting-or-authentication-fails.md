# 게시와 인증에 실패한 경우

성공 시에는 게시물 ID와 X의 URL을 반환합니다. 게시물 작성 통신 실패나 게시물 ID 누락은 미확정으로 반환하며, 자동으로 다시 보내지 않습니다. 같은 문구를 다시 게시하도록 요청한 경우에는 각 호출마다 한 번 전송합니다. 기록·본문 해시를 이유로 중지하는 구조는 없습니다.

일반 API 요청은 45초, 이미지 업로드는 이미지 1개당 120초가 기본 통신 시간 범위입니다. 이미지 업로드만은 일시적인 실패에 한해 같은 이미지를 최대 1회 재시도합니다. 대상은 HTTP 502·503·504(250ms 대기)와 프록시·업스트림 장애나 원인 불명의 JSON 거부, 통신 끊김(1초 대기)이며, 인증·잔액·이미지 오류·중복·속도 제한·타임아웃은 재시도하지 않습니다. 대기와 재시도는 이미지 1개의 통신 시간 범위 안에 맞추며, 이전 이미지의 재전송이나 게시물 작성·로그인의 자동 재전송은 하지 않습니다. 이미지 실패 시에는 게시물을 전송하지 않으며, 게시물 작성 후 성공 여부가 불명확해진 경우에는 `POST_UNCERTAIN`을 반환합니다.

알려진 게시 처리 오류에는 `stage`(`input_validation`·`pre_send`·`image_upload`·`post_creation`), 게시물 작성 요청을 시도했는지 나타내는 `postAttempted`, 실행한 이미지 업로드 요청의 총수 `uploadAttempts`를 포함합니다. 계정·Cookie 부족이나 중계 준비 실패는 `pre_send`입니다. 알 수 없는 내부 오류와 MCP 입력 형식 거부에서는 이 값들을 반환하지 않습니다. `postAttempted: true`는 게시 성공을 의미하지 않습니다. 제공처의 원본 오류나 자격 증명은 반환하지 않습니다.

이미지 통신의 `API_UNAVAILABLE`에는 `failureKind`를 포함하여 `timeout`(시간 초과)·`network`(통신 장애)·`cancelled`(취소)를 구분합니다. 이 경우에도 이미지나 게시물을 자동으로 다시 보내지 않습니다.

API의 거부 오류(`API_REJECTED`·`API_HTTP_ERROR`)에는 `httpStatus`, 고정 분류의 배열 `rejectionReasons`, 분류 출처 `rejectionReasonSource`를 포함합니다. 게시물 작성의 5xx가 `POST_UNCERTAIN`이 되는 경우에도 이 진단 정보를 유지합니다. 분류 출처는 `provider_message`(제공처의 JSON 메시지에서 추정한 후보) 또는 `http_status`(HTTP 상태에 따른 분류)입니다. 후보는 원인의 확정이나 재전송 허가를 의미하지 않으며, 판별할 수 없는 문구는 `unknown`으로 합니다. 원본 응답문·임의의 오류 코드·request ID는 반환하지 않으며 저장하지도 않습니다.

`provider_message`의 후보는 `api_key`·`authentication`·`account_locked`·`account_suspended`·`rate_limit`·`credits`·`proxy`·`media`·`duplicate`·`upstream_timeout`·`upstream_unavailable`·`automation_suspected`·`daily_limit`·`text_too_long`·`session_mismatch`·`post_id_missing`·`unknown`입니다. `unknown` 앞의 다섯 가지는 공개적으로 알려진 문구에 기반한 후보이며, 제공자가 반드시 이 문구로 반환한다고 확인된 것은 아닙니다. 이들은 재시도하지 않습니다. `post_id_missing`은 게시물이 작성되었을 수 있으므로, 다시 보내기 전에 계정에서 확인하세요. `http_status`에는 추가로 `invalid_request`(400)·`unauthorized`(401)·`forbidden`(403)·`endpoint_not_found`(404)가 있습니다. 예를 들어 이미지 업로드 시 `rejectionReasons: ["proxy", "upstream_timeout"]`과 `postAttempted: false`라면, 프록시·업스트림 타임아웃이 의심되며 게시물 작성 전에 멈췄다는 것을 알 수 있습니다. 2xx가 아닌 본문은 읽지 않으며, 기존 통신 상한·재시도 조건을 유지합니다.

API 응답이 10MiB를 넘으면 수신을 중단하고 실패를 반환합니다. 게시 응답에서 상한을 넘은 경우에도 이미 게시되었을 가능성이 있으므로 다시 보내지 않습니다.

저장된 Cookie의 대조 실패는 게시만 중지합니다. 공개 게시물 읽기·검색은 API 키와 사용자 이름만 사용하며, 저장된 Cookie를 읽지 않고 이용할 수 있습니다. 인증된 Cookie의 취득·갱신은 [설정 방법](../../configuration.md)를 참조하세요.

`login_account`는 저장된 Cookie가 사용 가능한 형식이면 재사용하며, X에서 유효성을 검증했다고는 보고하지 않습니다. 만료나 설정 불일치가 확인된 경우에는 `refresh_session`을 호출합니다. 새 Cookie는 Plumivox의 저장 위치에 Windows에서는 DPAPI, macOS에서는 Keychain의 키로 암호화하여 저장하며, 실패한 게시를 자동으로 다시 보내지 않습니다.

저장 교체 전에 중단한 경우에는 `AUTH_CANCELLED`, 인증 갱신의 배타 제어를 유지할 수 없는 경우에는 `AUTH_LOCK_LOST`, 저장할 수 없는 Cookie는 `LOGIN_FAILED`로 반환합니다. 저장 교체를 지시한 뒤 결과를 확인할 수 없는 경우에는 `AUTH_SAVE_UNCERTAIN`이 되며, 이미 갱신되었을 가능성이 있습니다. `SESSION_STORAGE_UNAVAILABLE`은 저장 위치나 Windows의 DPAPI·macOS의 Keychain 등 저장 처리의 실패를 나타냅니다.
