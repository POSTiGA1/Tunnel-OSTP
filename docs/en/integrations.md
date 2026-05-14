# Native Integrations

## Cross-Platform Engineering
The OSTP core protocol is developed to be completely platform-agnostic, operating uniformly across distinct operating systems. To interface with host-specific network stacks, integration layers are built to wrap the core asynchronous runtime.

## Mobile SDK
To support deployment on mobile platforms, the codebase includes a dedicated Native Development Kit (NDK) integration layer.
- **C-ABI Exposure**: The core functionalities are exported via a strictly typed C Application Binary Interface. This ensures compatibility with standard foreign function mechanisms required by high-level languages like Java, Kotlin, or Dart.
- **Isolated Runtimes**: The native module initializes and governs its own multithreaded asynchronous runtime within the host process memory. This architectural choice prevents heavy network I/O operations from interfering with or blocking the primary user interface thread of the mobile application.
- **Telemetry Bridges**: Memory-safe communication channels are established across the boundary, enabling the host application to poll connection telemetry and extract operational logs efficiently without risking concurrency faults or memory leaks.

## System Interfaces
On desktop environments, specialized modules govern the interaction with the operating system's routing subsystem. Depending on the operational mode, the integration layer safely manipulates process-level routing registries or binds directly to virtualized network driver adapters, providing seamless transparent traffic redirection.
