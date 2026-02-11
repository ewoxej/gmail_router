# Gmail Router

Автоматический роутер почты для Gmail, который фильтрует и удаляет письма на основе конфигурации. Программа фильтрует по адресу to, то есть имеет смысл если вы владете своим доменом и направляете почту
Добавить возможность пересылки на другие адреса через mailjet

## Настройка Gmail API

### 1. Создание проекта и включение Gmail API

1. Перейдите в [Google Cloud Console](https://console.cloud.google.com/)
2. Создайте новый проект или выберите существующий
3. Перейдите в "APIs & Services" → "Library"
4. Найдите "Gmail API" и нажмите "Enable"

### 2. Создание OAuth2 Credentials

1. Перейдите в "APIs & Services" → "Credentials"
2. Нажмите "Create Credentials" → "OAuth client ID"
3. Выберите "Desktop app" как тип приложения
4. Дайте название (например, "Gmail Router")
5. Нажмите "Create"
6. Скачайте JSON файл с credentials
7. Сохраните его как `credentials.json` в директории проекта

### 3. Настройка OAuth consent screen

1. Перейдите в "APIs & Services" → "OAuth consent screen"
2. Выберите "External" (если не используете Google Workspace)
3. Заполните обязательные поля (название приложения, email и т.д.)
4. Добавьте scope: `https://www.googleapis.com/auth/gmail.modify`
5. Добавьте себя в тестовые пользователи

## Установка и запуск

### 1. Клонирование и сборка

```bash
# Переход в директорию проекта
cd gmail_router

# Сборка проекта
cargo build --release
```

### 2. Настройка конфигурации

Скопируйте примеры конфигов и отредактируйте их:

```bash
cp credentials.yaml.example credentials.yaml
```

Отредактируйте `credentials.yaml`:

```yaml
google_credentials_path: "credentials.json"  # Путь к OAuth2 credentials
domain: "example.com"                        # Ваш домен
check_interval_seconds: 3600                 # Интервал проверки (1 час)
start_date: "2024-01-01T00:00:00Z"          # С какой даты проверять
```

### 3. Первый запуск (инициализация)

```bash
cargo run --release
```

При первом запуске:
1. Откроется браузер для авторизации в Google
2. Разрешите доступ приложению
3. Программа просканирует все письма и создаст `routing.yaml`
4. Все найденные адреса будут добавлены с флагом `true` (разрешены)

### 4. Настройка роутинга

Отредактируйте `routing.yaml`, чтобы заблокировать нужные адреса:

```yaml
addresses:
  admin: true      # Разрешено - письма не удаляются
  test: true       # Разрешено
  spam: false      # Заблокировано - письма удаляются
  abuse: false     # Заблокировано
```

### 5. Запуск в режиме демона

После настройки конфига просто запустите программу снова:

```bash
cargo run --release
```

Программа будет работать постоянно, проверяя почту каждые `check_interval_seconds` секунд.

## Запуск как systemd сервис (Linux)

Создайте файл `/etc/systemd/system/gmail-router.service`:

```ini
[Unit]
Description=Gmail Router Service
After=network.target

[Service]
Type=simple
User=your_username
WorkingDirectory=/path/to/gmail_router
ExecStart=/path/to/gmail_router/target/release/gmail_router
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Запустите сервис:

```bash
sudo systemctl daemon-reload
sudo systemctl enable gmail-router
sudo systemctl start gmail-router
sudo systemctl status gmail-router
```

## Логирование

По умолчанию уровень логирования - `info`. Для изменения установите переменную окружения:

```bash
# Debug уровень
RUST_LOG=debug cargo run --release

# Только ошибки
RUST_LOG=error cargo run --release

# Детальное логирование конкретного модуля
RUST_LOG=gmail_router::processor=debug cargo run --release
```
## Структура проекта

```
gmail_router/
├── src/
│   ├── main.rs          # Основная логика и цикл демона
│   ├── config.rs        # Работа с конфигами
│   ├── gmail.rs         # Gmail API клиент
│   └── processor.rs     # Обработка и фильтрация писем
├── Cargo.toml
├── credentials.yaml     # Конфиг с credentials (создается вручную)
├── routing.yaml         # Конфиг роутинга (создается автоматически)
└── token_cache.json     # OAuth токены (создается автоматически)
```

## Лицензия

MIT
