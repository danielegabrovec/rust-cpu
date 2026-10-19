# Fonti tecniche e significato dei dati

Fonti primarie consultate il **19 ottobre 2026**. I collegamenti descrivono le API e i provider usati; non attestano che ogni dispositivo o runtime sia stato provato fisicamente. Le prove eseguite per questa versione sono riportate separatamente nella documentazione di verifica.

## Telemetria di sistema

| Componente | Fonte | Uso nel programma |
| --- | --- | --- |
| Sistema, CPU, RAM, processi, dischi, rete, temperature | [sysinfo 0.39.6](https://docs.rs/sysinfo/0.39.6/sysinfo/) | Raccolta tramite le API offerte dal sistema operativo. I campioni CPU e le velocità I/O richiedono letture successive; l'istanza del raccoglitore viene mantenuta. |
| Identificazione CPU e cache x86/x86-64 | [raw-cpuid 11.6](https://docs.rs/raw-cpuid/11.6.0/raw_cpuid/) | Vendor, family/model/stepping, istruzioni, capacità di indirizzamento e cache quando esposte da CPUID. Il supporto a una feature non garantisce che il sistema operativo ne consenta l'uso. |
| Accesso WMI da Rust | [wmi 0.18.4](https://docs.rs/wmi/0.18.4/wmi/) | Inventario Windows in un thread dedicato con connessione COM appartenente a quel thread. |
| CPU e socket Windows | [Win32_Processor](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-processor) | Nome, socket, clock massimo dichiarato e cache firmware. Non viene letto il numero seriale né ProcessorId. |
| Scheda madre | [Win32_BaseBoard](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-baseboard) | Produttore, prodotto e revisione. |
| BIOS/UEFI | [Win32_BIOS](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-bios) | Produttore, versione e data. |
| Moduli RAM | [Win32_PhysicalMemory](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-physicalmemory) | Capacità, slot, produttore, part number, tipo SMBIOS, velocità e tensione configurate dal firmware. Non è una lettura diretta del chip SPD. |
| Array di memoria | [Win32_PhysicalMemoryArray](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-physicalmemoryarray) | Slot ed ECC dichiarati dal firmware; non è una diagnosi degli errori fisici della memoria. |
| Inventario GPU Windows | [Win32_VideoController](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-videocontroller) | Nome adattatore, driver e processore video. La proprietà AdapterRAM non è usata per misurare VRAM moderna. |
| Sensori NVIDIA | [NVIDIA System Management Interface](https://docs.nvidia.com/deploy/nvidia-smi/) | Query delle colonne nome, utilizzo, memoria, temperatura e potenza tramite `nvidia-smi`. I campi non supportati restano assenti. |

I contatori sono esposti in byte e byte/s; le dimensioni leggibili usano KiB, MiB, GiB e TiB. La CPU di un processo viene normalizzata rispetto a tutti i processori logici della macchina. La somma RSS dei processi LLM è un indicatore: può includere pagine condivise e non coincide con il consumo esclusivo dei pesi del modello.

Su Windows, l'inventario esteso deriva da WMI/SMBIOS. I valori firmware possono essere mancanti o errati. Per clock effettivi per core, tensioni CPU, timing della RAM, lettura SPD diretta e sensori proprietari servono provider ulteriori: il programma non inventa quei valori. Il supporto di una piattaforma da parte di una dipendenza non equivale alla verifica della presente applicazione su quella piattaforma.

## Runtime LLM

| Runtime / operazione | Fonte ufficiale | Comportamento implementato |
| --- | --- | --- |
| Ollama, modelli installati | [GET /api/tags](https://docs.ollama.com/api/tags) | Nome, dimensioni e quantizzazione; modelli noti come remoti esclusi. |
| Ollama, modelli caricati | [GET /api/ps](https://docs.ollama.com/api/ps) | Stato caricato, memoria GPU e contesto dichiarato dal runtime. Il contesto è quello riportato dal runtime, non una prova di occupazione attuale. |
| Ollama, benchmark | [POST /api/generate](https://docs.ollama.com/api/generate) | Stream NDJSON; token e durate native in nanosecondi. Il prefill sottrae i token di cache quando il contatore è disponibile; altrimenti la sua velocità resta n/d. |
| Ollama, metadati remoti | [Schema API ufficiale](https://github.com/ollama/ollama/blob/main/docs/openapi.yaml) | Riconoscimento di `remote_host` e `remote_model`. Anche i nomi che contengono `cloud` vengono esclusi in modo conservativo. |
| LM Studio, elenco nativo | [GET /api/v1/models](https://lmstudio.ai/docs/developer/rest/list) | Modelli di tipo LLM, dimensioni, quantizzazione, istanze caricate e contesto. Quando esistono più istanze viene mostrata la prima. |
| LM Studio, compatibilità | [Chat Completions](https://lmstudio.ai/docs/developer/openai-compat/chat-completions) | Benchmark su `/v1/chat/completions`, stream SSE. L'elenco OpenAI `/v1/models` è un ripiego per server che non espongono la rotta nativa. |
| llama.cpp | [Documentazione del server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) | `/health`, `/v1/models` e chat completions; HTTP 503 segnala un server non pronto. Quando presenti vengono usati i contatori e tempi nativi `timings`. |

La scoperta usa soltanto richieste di metadati e non avvia inferenza, download o caricamento intenzionale. Un elenco OpenAI non distingue necessariamente modelli caricati e modelli disponibili: il programma conserva `loaded = null` quando il protocollo non fornisce questa informazione.

Per i benchmark compatibili OpenAI, quando manca un tempo di decode nativo, la velocità è **token di output dichiarati da `usage` / durata totale misurata dal client**. Comprende prefill, eventuale caricamento e coda: non è direttamente confrontabile con il decode nativo Ollama. Senza `usage` o contatori nativi non si stimano token contando caratteri; conteggio e velocità restano n/d.

## Interfaccia e HTTP

- [Ratatui: sviluppo di applicazioni](https://ratatui.rs/recipes/apps/): rendering terminale, ciclo eventi e gestione dello stato.
- [reqwest 0.12.28 ClientBuilder](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html): timeout, proxy e redirect. L'applicazione disabilita proxy e redirect e accetta soltanto origini HTTP loopback.

Le pagine dei runtime evolvono: per un errore di compatibilità, allegare versione del runtime, codice HTTP e identificativo del modello evitando chiavi e conversazioni private. I test con server HTTP locali simulati verificano il client, ma non sostituiscono una misura con il proprio modello e hardware.
