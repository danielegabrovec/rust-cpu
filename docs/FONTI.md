# Fonti tecniche e significato dei dati — 0.2

Riferimenti primari della versione 0.2, aggiornati il **19 ottobre 2026**. Le
versioni Rust indicate corrispondono alle dipendenze del progetto. Una pagina
API descrive un contratto; non sostituisce le prove su hardware e runtime reali,
registrate in [VERIFICHE.md](VERIFICHE.md). Lo storico 0.1 resta distinto.

## Sistema, CPU e inventario Windows

| Componente | Fonte primaria | Impiego e limite |
| --- | --- | --- |
| Telemetria OS | [sysinfo 0.39.6](https://docs.rs/sysinfo/0.39.6/sysinfo/) | CPU, RAM, processi, I/O, rete e sensori esposti dal sistema; rate da letture successive. |
| CPU x86/x86-64 | [raw-cpuid 11.6.0](https://docs.rs/raw-cpuid/11.6.0/raw_cpuid/) | Identificazione, feature e cache riportate; non certifica istruzioni effettivamente usate dal runtime. |
| WMI Rust | [wmi 0.18.4](https://docs.rs/wmi/0.18.4/wmi/) | Connessione creata e usata nel thread COM del provider. |
| CPU/socket | [Win32_Processor](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-processor) | Socket, clock e cache dichiarati dal firmware; niente ProcessorId o seriale. |
| Scheda madre | [Win32_BaseBoard](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-baseboard) | Produttore, prodotto, revisione. |
| Firmware | [Win32_BIOS](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-bios) | Produttore, versione e data BIOS/UEFI. |
| DIMM | [Win32_PhysicalMemory](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-physicalmemory) | Slot, capacità, tipo, velocità e tensione configurate; non lettura diretta SPD. |
| Array RAM | [Win32_PhysicalMemoryArray](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-physicalmemoryarray) | Slot ed ECC dichiarati; non test degli errori fisici. |
| GPU di inventario | [Win32_VideoController](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-videocontroller) | Nome e driver. `AdapterRAM` non viene usata per misurare capacità VRAM moderne. |

Byte e byte/s restano unità dei contatori; la presentazione usa KiB, MiB, GiB e
TiB. La CPU di un processo è normalizzata sulla capacità logica totale. Frequenze
OS e valori firmware possono essere nominali o errati; non vengono presentati
come clock effettivo di ciascun core. Il riconoscimento dei processi LLM è
un'euristica sul nome e non dimostra la proprietà di una specifica conversazione.

## GPU Windows e sensori

| Fonte primaria | Uso in RUST-CPU |
| --- | --- |
| [GPU nel Task Manager, Microsoft DirectX](https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/) | Significato WDDM del carico per motore e della memoria dedicata/condivisa. |
| [PdhAddEnglishCounterW](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhaddenglishcounterw) | Percorsi contatori indipendenti dalla lingua Windows. |
| [PdhGetFormattedCounterArrayW](https://learn.microsoft.com/en-us/windows/win32/api/pdh/nf-pdh-pdhgetformattedcounterarrayw) | Lettura delle istanze wildcard e dei loro valori/stati. |
| [IDXGIAdapter::GetDesc](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgiadapter-getdesc) | Descrizione adattatore e capacità memoria del driver; correlazione interna tramite LUID. |
| [Problema noto dei contatori GPU Process Memory](https://learn.microsoft.com/en-us/troubleshoot/windows-client/performance/gpu-process-memory-counters-report-wrong-value) | Limite documentato su alcune versioni Windows 10; non assumere infallibile la memoria attribuita per PID. |
| [NVIDIA System Management Interface](https://docs.nvidia.com/deploy/nvidia-smi/) | Temperatura, potenza e fallback NVIDIA; campi non supportati rimangono assenti. |
| [Esempio WMI ufficiale LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/blob/master/LibreHardwareMonitor.Windows.Forms/TestScripts/basicwmi.py) | Namespace opzionale del programma già avviato; nessuna installazione o attivazione da RUST-CPU. |

Il carico dell'adattatore usa il motore più occupato; i contributi dei PID si
aggregano sullo stesso motore, senza sommare motori differenti oltre il 100%.
La memoria dedicata resta separata dalla RAM condivisa. Le attribuzioni di una
risorsa condivisa possono apparire in più PID: la loro somma non rappresenta
necessariamente memoria fisica aggiuntiva.

`GpuRow.source` conserva il provider primario del carico. Le prove 0.2 hanno
rilevato carico e memoria WDDM reali su AMD e NVIDIA del PC di sviluppo. Il provider
LHM non era attivo: temperatura/tensione LHM sono verificate con fixture,
non con letture CPU reali. Copertura e scadenze sono in
[HARDWARE.md](HARDWARE.md). SPD diretto, timing RAM e sensori non pubblicati
restano fuori dalla copertura senza un provider specifico.

## Archivio SQLite e statistiche

| Fonte primaria | Uso nel progetto |
| --- | --- |
| [rusqlite 0.40.2](https://docs.rs/rusqlite/0.40.2/rusqlite/) | Connessione SQLite bundled e parametri associati. |
| [Transaction](https://docs.rs/rusqlite/0.40.2/rusqlite/struct.Transaction.html) | Avvio sessione/primo campione e import atomici con rollback su errore. |
| [Backup](https://docs.rs/rusqlite/0.40.2/rusqlite/backup/struct.Backup.html) | Copia online a passi, con deadline applicativa e gestione Busy/Locked. |
| [SQLite: WAL](https://www.sqlite.org/wal.html) | Accesso da processi locali concorrenti e gestione dei file WAL. |
| [SQLite: synchronous](https://www.sqlite.org/pragma.html#pragma_synchronous) | Configurazione FULL scelta dall'applicazione. |
| [SQLite: application_id](https://www.sqlite.org/pragma.html#pragma_application_id) e [user_version](https://www.sqlite.org/pragma.html#pragma_user_version) | Riconoscimento archivio e versione di schema. |

Le soglie di coda, i cinque secondi di shutdown/backup, i limiti di import e gli
algoritmi dei riepiloghi sono scelte di RUST-CPU, documentate in
[ARCHITETTURA.md](ARCHITETTURA.md), non garanzie aggiunte dalla libreria SQLite.

Media e picco sono calcolati su tutti i valori salvati presenti. Il P95 usa il
rango `ceil(0,95 × N)` ordinato, senza interpolazione. La media è per campione,
non pesata sul tempo. Anteprime distribuite preservano gli estremi ma non
sostituiscono i dati statistici completi. Il confronto esportato dalla TUI è lo
stesso oggetto visualizzato, anche se la sessione riceve altri campioni dopo il
caricamento.

Versione originale dell'app e provider GPU sono conservati. I confronti avvisano
quando provenienza o metodo sono diversi o mancanti; un delta percentuale con
base zero resta n/d. Un uso inferiore della CPU non costituisce da solo un
risultato migliore sul medesimo lavoro.

## API LLM e autenticazione

| Runtime/operazione | Fonte primaria | Uso implementato |
| --- | --- | --- |
| Ollama installato | [GET /api/tags](https://docs.ollama.com/api/tags) | Modelli disponibili, dimensioni e quantizzazione. |
| Ollama caricato | [GET /api/ps](https://docs.ollama.com/api/ps) | Modelli caricati e dati dichiarati dal runtime. |
| Ollama generazione | [POST /api/generate](https://docs.ollama.com/api/generate) | NDJSON, conteggi e durate native; prefill non in cache quando verificabile. |
| Ollama schema | [OpenAPI ufficiale](https://github.com/ollama/ollama/blob/main/docs/openapi.yaml) | Campi come `remote_host` e `remote_model`, usati per escludere instradamenti remoti noti dal benchmark sintetico. |
| LM Studio modelli | [GET /api/v1/models](https://lmstudio.ai/docs/developer/rest/list) | LLM, quantizzazione e istanze caricate; scelta della prima istanza quando ne esistono più. |
| LM Studio chat | [Chat Completions](https://lmstudio.ai/docs/developer/openai-compat/chat-completions) | Endpoint compatibile OpenAI e SSE. |
| LM Studio autenticazione | [Authentication](https://lmstudio.ai/docs/developer/core/authentication) | Token Bearer per server che richiedono autenticazione. |
| llama.cpp | [README server ufficiale](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) | Health, modelli, chat, autenticazione configurata dal server e tempi nativi quando presenti. |

Le sonde non avviano inferenza. Un protocollo che non espone lo stato di
caricamento conserva `loaded = null`. `native_decode` usa conteggi e tempi nativi;
`end_to_end` usa output token dichiarati divisi per la durata del client/proxy.
Non vengono equiparati. In assenza di contatori reali, conteggio e throughput
restano n/d; non si ricavano token dal numero dei caratteri.

La lettura delle credenziali è una scelta applicativa: `RUST_CPU_OLLAMA_TOKEN`,
`RUST_CPU_LM_STUDIO_TOKEN`, `RUST_CPU_LLAMA_CPP_TOKEN` e frontend
`RUST_CPU_OBSERVER_TOKEN`. Il proxy richiede protezione frontend quando l'upstream
ha un token configurato. Token e header privati non vengono persistiti né inclusi
nei report. Istruzioni d'uso e limiti sono in [OBSERVER.md](OBSERVER.md).

## TUI, HTTP e verifica

- [Ratatui](https://ratatui.rs/recipes/apps/): rendering terminale e gestione dello stato.
- [reqwest 0.12.28 ClientBuilder](https://docs.rs/reqwest/0.12.28/reqwest/struct.ClientBuilder.html): timeout e configurazione del client HTTP.
- [Axum 0.8.9](https://docs.rs/axum/0.8.9/axum/): server HTTP dell'observer.
- [Tokio 1.53.3](https://docs.rs/tokio/1.53.3/tokio/): runtime asincrono, sincronizzazione e arresto del trasporto.

Le restrizioni loopback, l'assenza di proxy di ambiente/redirect, i limiti dei corpi
e la mancata persistenza di prompt e risposte sono verificati dall'applicazione.
Le prove HTTP 0.2 comprendono 32 casi locali: 20 per il client LLM e 12 per
l'observer. La suite non equivale alla prova fisica di tutte le versioni di runtime.

Il microbenchmark CPU mix64 v1 è definito in `src/bench.rs`: non possiede una
scala CPU-Z e va confrontato con lo stesso algoritmo e profilo di build. I
benchmark LLM osservati sono distinti dai test sintetici; nei gruppi omogenei
vengono riportati valori medi e, quando previsti, mediana e deviazione standard.
Nessun risultato conserva il testo delle richieste o delle risposte.
