# open-rally

여러 PC 사이에서 값과 파일을 주고받기 위한 로그인 없는 공용 게시판. 개인 Mac 의 메뉴바에서 켜고 끄고, 같은 LAN 이나 Tailscale 안의 PC 가 브라우저로 접속한다.

## 빌드와 실행

```bash
bash scripts/bundle-app.sh     # dist/open-rally.app 생성 (release 빌드)
open dist/open-rally.app       # 메뉴바에 아이콘이 뜨고 서버가 켜진다
```

메뉴바 아이콘 메뉴에서 **서버 켜기/끄기**, **브라우저에서 열기**, **접속 주소 복사**, **LAN 에도 공개**, **비밀번호 파일 열기**, **데이터 폴더 열기**, **종료** 를 할 수 있다. 채워진 원이 켜짐, 빈 원이 꺼짐이다.

트레이 없이 서버만 띄우려면:

```bash
cargo run -- --serve          # Ctrl+C 로 종료, --lan 을 붙이면 LAN 에도 공개
```

## 접속과 비밀번호

다른 PC 에서 메뉴의 Tailscale 주소(`http://100.x.x.x:7777`)를 브라우저로 연다. 처음 한 번 비밀번호를 입력하면 30일 동안 기억한다. 서버를 껐다 켜면 다시 묻는다.

비밀번호는 데이터 폴더의 `config.toml` 에 있다. 첫 실행 때 무작위로 만들어지며, 메뉴의 "비밀번호 파일 열기" 로 바꾼 뒤 서버를 껐다 켜면 반영된다.

```
~/Library/Application Support/open-rally/
  config.toml        password = "..."
  open-rally.db      글 목록 (SQLite)
  uploads/<글 id>/   첨부 파일
```

## 환경변수

| 이름 | 기본값 | 설명 |
|---|---|---|
| `OPEN_RALLY_PORT` | `7777` | 리스닝 포트 |
| `OPEN_RALLY_DATA_DIR` | `~/Library/Application Support/open-rally` | 데이터 폴더 |

## 보안 전제

- 기본은 **Tailscale 인터페이스와 localhost 에만** 열린다. 카페 Wi-Fi 에 붙어도 같은 망의 다른 기기에서는 보이지 않는다.
- "LAN 에도 공개" 를 켜면 모든 인터페이스에 열린다. 이 설정은 저장되지 않고 앱을 다시 켜면 꺼진다.
- HTTP 평문이다. 비밀번호는 같은 망의 누군가가 우연히 보는 것을 막는 수준이며, 패킷을 보는 사람은 막지 못한다.
- 브라우저 경유 공격(CSRF, DNS rebinding) 은 Host/Origin 허용 목록으로 막는다. SVG 와 HTML 첨부는 미리보기 없이 다운로드만 된다.

## 개발

```bash
cargo test
```
