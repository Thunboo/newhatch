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

После обновления capture filter выполните:

```bash
curl "http://localhost:18080/flag?player=team01&payload=demo-exploit"
```

Query string имитирует данные, отправленные игроком. В найденной session поток C2S содержит HTTP-запрос с `player` и `payload`, а S2C содержит JSON с флагом. Строка флага подходит как для regex `FLAG\{[^}\r\n]+\}`, так и для compose-default `FLAG_[A-Za-z0-9]+`.

Остановить тестовый сервис:

```bash
docker compose --profile test stop test-flag
```
