#!/bin/bash
set -e

# Официальный репозиторий
GITHUB_REPO="ospab/ostp"
INSTALL_DIR="/opt/ostp"

echo "========================================================"
echo " Установка Ospab Stealth Transport Protocol (OSTP)"
echo "========================================================"

# Проверка прав суперпользователя
if [ "$EUID" -ne 0 ]; then
  echo "[Ошибка] Данный скрипт должен быть запущен с правами root (sudo)."
  exit 1
fi

# Создание директории
mkdir -p "$INSTALL_DIR"

# ---------------------------------------------------------
# Определение архитектуры системы
# ---------------------------------------------------------
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    i386|i686) ARCH="386" ;;
    armv7l) ARCH="armv7" ;;
    *) 
        echo "[Предупреждение] Неизвестная архитектура $ARCH, пробуем amd64 по умолчанию."
        ARCH="amd64"
        ;;
esac

# Скачивание исполняемого файла
echo "Получение актуальной стабильной версии из репозитория..."
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST_RELEASE" ] || [[ "$LATEST_RELEASE" == *"null"* ]]; then
   echo "[Уведомление] Не удалось автоматически получить тег репозитория ${GITHUB_REPO}."
   echo "Введите прямую ссылку (URL) на скомпилированный архив .tar.gz"
   echo "или нажмите Enter, если файл уже находится в $INSTALL_DIR/ostp."
   read -p "URL: " DIRECT_URL
   if [ -n "$DIRECT_URL" ]; then
      TEMP_TAR="/tmp/ostp_temp.tar.gz"
      curl -L "$DIRECT_URL" -o "$TEMP_TAR"
      tar -xzf "$TEMP_TAR" -C "$INSTALL_DIR" ostp
      rm "$TEMP_TAR"
   fi
else
   ARCHIVE_NAME="ostp-linux-${ARCH}.tar.gz"
   DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_RELEASE}/${ARCHIVE_NAME}"
   echo "Скачивание архива для архитектуры linux-$ARCH: $DOWNLOAD_URL ..."
   
   TEMP_TAR="/tmp/ostp_temp.tar.gz"
   # Скачиваем архив с обработкой ошибок
   HTTP_CODE=$(curl -sL -w "%{http_code}" "$DOWNLOAD_URL" -o "$TEMP_TAR")
   
   if [ "$HTTP_CODE" -eq 200 ]; then
      tar -xzf "$TEMP_TAR" -C "$INSTALL_DIR" ostp
      rm -f "$TEMP_TAR"
   else
      echo "[Ошибка] Не удалось скачать файл (HTTP код $HTTP_CODE)."
      echo "Убедитесь, что версия $LATEST_RELEASE уже опубликована и собрана на GitHub."
      rm -f "$TEMP_TAR"
      exit 1
   fi
fi

if [ -f "$INSTALL_DIR/ostp" ]; then
   chmod +x "$INSTALL_DIR/ostp"
   echo "Исполняемый файл настроен в $INSTALL_DIR/ostp."
else
   echo "[Ошибка] Бинарный файл не обнаружен в $INSTALL_DIR/ostp. Прекращение настройки."
   exit 1
fi

# Интерактивный выбор режима
echo "--------------------------------------------------------"
echo "Выберите режим конфигурации:"
echo "1) Настройка Сервера"
echo "2) Настройка Клиента"
echo "--------------------------------------------------------"
read -p "Введите номер [1-2]: " NODE_MODE

cd "$INSTALL_DIR"

if [ "$NODE_MODE" == "1" ]; then
  echo "Инициализация конфигурации сервера..."
  # Используем внутренний инструмент --init для создания шаблона
  ./ostp --init server --config config.json
  
  read -p "Укажите IP и порт для приема входящего трафика [по умолчанию 0.0.0.0:50000]: " LISTEN_ADDR
  if [ -n "$LISTEN_ADDR" ]; then
     sed -i "s/\"listen\": \"0.0.0.0:50000\"/\"listen\": \"$LISTEN_ADDR\"/g" config.json
  fi
  
  read -p "Сколько ключей авторизации сгенерировать? [по умолчанию 1]: " KEYS_COUNT
  KEYS_COUNT=${KEYS_COUNT:-1}
  
  if [ "$KEYS_COUNT" -gt 1 ]; then
     echo "Генерация дополнительных ключей безопасности..."
     NEW_KEYS=$(./ostp -g -c "$KEYS_COUNT" | sed 's/^/      "/;s/$/",/' | sed '$ s/,$//')
     # Заменяем весь блок access_keys в JSON
     sed -i '/"access_keys": \[/,/\]/c\  "access_keys": [\n'"$NEW_KEYS"'\n  ],' config.json
     echo "Сгенерировано и записано $KEYS_COUNT ключей."
  fi
  echo "Настройка сервера завершена. Файл: $INSTALL_DIR/config.json"

elif [ "$NODE_MODE" == "2" ]; then
  echo "Инициализация конфигурации клиента..."
  ./ostp --init client --config config.json
  
  read -p "Введите адрес внешнего сервера (IP:PORT): " REMOTE_SERVER
  if [ -n "$REMOTE_SERVER" ]; then
     sed -i "s/\"server\": \"127.0.0.1:50000\"/\"server\": \"$REMOTE_SERVER\"/g" config.json
  else
     echo "[Предупреждение] Адрес не указан, оставлено значение по умолчанию (127.0.0.1:50000)."
  fi
  
  read -p "Введите ключ авторизации (оставьте пустым для генерации нового через ostp -g): " ACCESS_KEY
  if [ -z "$ACCESS_KEY" ]; then
     ACCESS_KEY=$(./ostp -g)
     echo "Автоматически сгенерирован ключ клиента: $ACCESS_KEY"
  fi
  # Заменяем значение ключа в JSON
  sed -i "s/\"access_key\": \"[^\"]*\"/\"access_key\": \"$ACCESS_KEY\"/g" config.json

  read -p "Укажите локальный SOCKS5 адрес прослушивания [по умолчанию 127.0.0.1:1088]: " SOCKS_BIND
  if [ -n "$SOCKS_BIND" ]; then
     sed -i "s/\"socks5_bind\": \"127.0.0.1:1088\"/\"socks5_bind\": \"$SOCKS_BIND\"/g" config.json
  fi
  echo "Настройка клиента завершена. Файл: $INSTALL_DIR/config.json"

else
  echo "[Ошибка] Указан неверный вариант выбора."
  exit 1
fi

# Регистрация Systemd службы
echo "Настройка системного сервиса..."
cat <<EOF > /etc/systemd/system/ostp.service
[Unit]
Description=Ospab Stealth Transport Protocol Service
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/ostp --config $INSTALL_DIR/config.json
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable ostp.service >/dev/null 2>&1

echo "--------------------------------------------------------"
echo "Установка успешно завершена."
echo "Конфигурация сохранена в $INSTALL_DIR/config.json"
echo "Сервис ostp зарегистрирован, но не запущен."
echo "Запустите сервис вручную: systemctl start ostp"
echo "--------------------------------------------------------"
