# OSTP documentation

Every document exists in English (`en/`) and Russian (`ru/`) with the same content. Start with the [README](../README.md) for installation and a quick start.

| Topic | English | Русский | What is in it |
|---|---|---|---|
| Client | [client.md](en/client.md) | [client.md](ru/client.md) | Proxy and TUN modes, transports, recovery and roaming, exclusions, multiplexing |
| Server | [server.md](en/server.md) | [server.md](ru/server.md) | Session dispatcher, attack mitigation, roaming, management API, DNS, fallback listener, outbound chaining |
| Domains and TLS | [tls.md](en/tls.md) | [tls.md](ru/tls.md) | TLS on 443, how the server tells connections apart, `ostp cert`, web servers, renewal, subscriptions |
| Protocol specification | [specification.md](en/specification.md) | [specification.md](ru/specification.md) | Wire format, header masking, key derivation and handshake, frames, ARQ, congestion control, roaming |
| Obfuscation | [obfuscation.md](en/obfuscation.md) | [obfuscation.md](ru/obfuscation.md) | Why OSTP looks like noise: secret derivation, masking, padding, junk packets, fragmentation |
| Architecture | [architecture.md](en/architecture.md) | [architecture.md](ru/architecture.md) | How the crates fit together, envelope vs. frame, multiplexing, ARQ, roaming, server subsystems |
| Integrations | [integrations.md](en/integrations.md) | [integrations.md](ru/integrations.md) | Android app and JNI, desktop GUI, system interfaces |
| Test protocol | [testing.md](en/testing.md) | [testing.md](ru/testing.md) | Checklist run before every beta and stable release |

Also here:

- [relay-config-example.json](relay-config-example.json): a commented config for a relay node that forwards clients to another server.
- [CHANGELOG.md](../CHANGELOG.md) / [CHANGELOG.ru.md](../CHANGELOG.ru.md): what changed in each release, also shown by `ostp changelog`.
- [CONTRIBUTING.md](../CONTRIBUTING.md) / [CONTRIBUTING.ru.md](../CONTRIBUTING.ru.md): repository layout, branches, how to send changes.
- The [wiki](https://github.com/ospab/ostp/wiki): installation, configuration reference, share links, FAQ.

---

# Документация OSTP

Каждый документ есть на английском (`en/`) и русском (`ru/`) с одинаковым содержанием. Установка и быстрый старт — в [README](../README.ru.md).
Таблица документов — выше. Кроме неё:

- [relay-config-example.json](relay-config-example.json): пример конфига узла-ретранслятора с комментариями.
- [CHANGELOG.ru.md](../CHANGELOG.ru.md): что изменилось в каждом выпуске; то же показывает `ostp changelog`.
- [CONTRIBUTING.ru.md](../CONTRIBUTING.ru.md): устройство репозитория, ветки, как присылать изменения.
- [Wiki](https://github.com/ospab/ostp/wiki): установка, справочник по конфигу, ссылки для подключения, FAQ.
