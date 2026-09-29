# Flag Capture Test

Тестовый nginx слушает порт `18080`. Единственный полезный endpoint `GET /flag` возвращает JSON с тестовым флагом:

```text
FLAG{FLAG_TEST_STOLEN_18080}
```

## Запуск

```bash
docker compose --profile test up -d test-flag
```

В newhatch откройте Sources и добавьте включенный source с TCP-портом `18080`, например `flag-test`.

Для проверки VPN-интерфейса отправляйте запрос с другого VPN-узла на VPN-адрес vulnbox. Например, если трафик приходит на `wt0`, установите `CAPTURE_INTERFACE=wt0`, пересоздайте analyzer через `docker compose up -d --build --force-recreate analyzer`, а `curl` запускайте с другой машины. Запрос с vulnbox к собственному адресу может пройти через `lo` и не проверяет входящий VPN-трафик.

После обновления capture filter выполните:

```bash
curl "http://localhost:18080/flag?player=team01&payload=demo-exploit"
```

Команда с `localhost` проверяет захват на `lo`/Docker bridge. Она не является проверкой `ens3` или VPN-интерфейса. Фактический путь можно увидеть командой `sudo tcpdump -ni any 'tcp port 18080'`.

Query string имитирует данные, отправленные игроком. В найденной session поток C2S содержит HTTP-запрос с `player` и `payload`, а S2C содержит JSON с флагом. Строка флага подходит как для regex `FLAG\{[^}\r\n]+\}`, так и для compose-default `FLAG_[A-Za-z0-9]+`.

Остановить тестовый сервис:

```bash
docker compose --profile test stop test-flag
```

## Request Export Regression Tests (Issue #12)

The export suite uses this fixture's existing nginx configuration and flag JSON.
It mocks only analyzer API responses in the browser. Generated Bash/cURL and
Python/requests snippets are actually executed against nginx inside an isolated
test image. It does not require live capture or a production analyzer.

Run from the repository root:

```bash
docker build -f test/flag_test/replay.Dockerfile -t newhatch-replay-test .
docker run --rm --shm-size=256m newhatch-replay-test
```

Agents must not run these commands: Docker daemon operations can interrupt the
user's VPN. The suite was last reported passing by the user on 2026-09-29:
20 Playwright tests passed, including all request-export regressions.

The build includes frontend type checking and a production build. The test run
starts Vite, nginx on container port 18080 for /flag, and a separate nginx proxy
on container port 18081 backed by a small loopback request-inspection server.
It publishes no host ports and mounts no production data. A running test-flag
Compose service is not needed.

Coverage includes:

- Bundled cURL/Python logos, text fallbacks and hover labels immediately before
  C2S ordinary copy.
- Both exported GET snippets receive the fixture's expected flag.
- POST/PUT replay preserves the displayed body, readable Unicode, compact/pretty
  JSON, literal quotes, shell metacharacters, leading @ and trailing newlines.
- Received headers, cookies, query arguments with repeated names, destination
  Host and recalculated UTF-8 Content-Length are checked through nginx.
- Unsupported framing/encoding, multiple requests, duplicate headers and Hex
  mode disable export while ordinary copy still works.
- Russian labels, mobile layout and the clipboard fallback.
- The existing auth, localization and session-feed browser regression suite.

The exported Bash script requires cURL 7.87+ for --url-query. The Python script
requires Python 3 and requests. All test dependencies are installed only in the
test image; production frontend/backend dependencies are unchanged.
