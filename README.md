# RUST-CPU 0.2

**Hardware, Local AI & SQLite Session Lab** — una TUI Rust per conoscere il PC,
seguire le risorse durante un lavoro LLM e confrontare sessioni salvate. Interfaccia
italiana, tredici viste, navigazione da tastiera, temi chiaro/scuro, grafici e archivio
locale. Prima piattaforma: **Windows x64**.

Software indipendente della categoria diagnostica hardware: non è affiliato a CPUID
o CPU-Z e non ne include codice, punteggi o benchmark proprietari.

![Overview RUST-CPU con dati dimostrativi](docs/screenshots/overview.png)

Le immagini pubbliche contengono **dati sintetici dichiarati**, non report del PC
dell'autore. Altre viste: [Sessioni](docs/screenshots/sessioni.png),
[Impostazioni](docs/screenshots/impostazioni.png), [LLM](docs/screenshots/llm.png),
[Processi](docs/screenshots/processi.png), [Capacity Lab](docs/screenshots/capacity.png).

## Cosa offre la versione 0.2

- **Archivio SQLite:** sessioni nominate, campioni, benchmark, avvisi, import/export e backup.
- **Replay e confronto A/B:** statistiche su tutti i campioni salvati, grafici a
  progressione normalizzata e gruppi di benchmark con metodo compatibile.
- **Preferenze persistenti e soglie:** tema, intervallo, ordinamento, Capacity Lab,
  URL dei runtime, ripetizioni e avvisi locali.
- **GPU Windows NVIDIA/AMD/Intel:** carico e memoria dedicata/condivisa tramite
  WDDM/DXGI quando esposti; attribuzioni GPU per processo. NVIDIA può aggiungere
  temperatura e potenza tramite `nvidia-smi`.
- **LLM locali:** scoperta di Ollama, LM Studio e llama.cpp, benchmark volontari
  singoli o ripetuti, autenticazione Bearer da variabili d'ambiente.
- **Observer facoltativo:** proxy loopback per misurare le richieste che il client
  sceglie di instradare attraverso RUST-CPU, mantenendole distinte dai benchmark.

SQLite è incorporato nell'eseguibile: **non occorre installare un server database
né `sqlite3.exe`**. Il normale monitoraggio non esegue inferenza; il benchmark
richiede un comando esplicito. Il proxy inoltra soltanto le richieste dei client
configurati per usarlo.

## Installazione e primo avvio

Scarica installer o ZIP portable da [Releases](https://github.com/danielegabrovec/rust-cpu/releases).
L'installer opera per l'utente corrente e aggiunge collegamenti al menu Start;
non richiede l'avvio come amministratore. Il setup non è firmato digitalmente.

Usa un terminale moderno con font monospaziato Unicode, per esempio Windows Terminal.
Consigliate almeno **100×32 celle**; è disponibile una disposizione compatta a 64×18.
La sidebar accetta clic sui terminali ampi, la rotella scorre e tutte le funzioni
principali restano raggiungibili da tastiera.

```powershell
# Nella cartella estratta dalla versione portable
.\rust-cpu.exe

# Un report hardware, senza TUI e senza interrogare le API LLM
.\rust-cpu.exe --snapshot .\report-001.json --no-llm

# Una breve sessione persistente di tre campioni
.\rust-cpu.exe --database .\archivio.sqlite3 --save-session "prima-prova" --samples 3 --no-llm

# Elenco delle sessioni e aiuto completo
.\rust-cpu.exe --database .\archivio.sqlite3 --sessions
.\rust-cpu.exe --help
```

Il database predefinito è `%LOCALAPPDATA%\RUST-CPU\workspace.sqlite3`;
`--database` permette un percorso diverso. I report interattivi vengono scritti in
`%USERPROFILE%\Documents\RUST-CPU\exports`, modificabile con `--export-dir`.
I file di output esistenti non vengono sovrascritti.

## Tredici viste

| Vista | Contenuto |
|---|---|
| Overview | CPU, RAM, GPU, grafici, thread logici e riepilogo delle risorse |
| CPU | Identità, cache CPUID, istruzioni, frequenza, sensori disponibili e microbenchmark |
| Mainboard | Scheda madre e firmware esposti da WMI/SMBIOS |
| Memoria | RAM, swap, moduli, capacità e configurazione dichiarata dal firmware |
| GPU | Adattatori, provider, carico, memoria dedicata/condivisa e sensori disponibili |
| Dischi / Rete | Spazio, velocità I/O e traffico per interfaccia |
| Processi | CPU/RSS/I/O, attribuzioni GPU, filtro, ordinamento e dettaglio del campione |
| LLM locali | Runtime, modelli, ultimo benchmark, osservazioni e cronologia SQLite |
| Capacity Lab | Stima modificabile di pesi, KV cache e margini; RAM/VRAM separate |
| Diagnostica | Fonti, sensori, avvisi dei collettori e limiti delle misure |
| Sessioni · F2 | Archivio persistente, nomi, riepiloghi, selezione A/B e replay |
| Confronto · F3 | Delta, statistiche, progressione e confronto dei benchmark |
| Impostazioni · F4 | Preferenze salvate, soglie, ripetizioni, percorso DB e avvisi |

## Registrare e confrontare un lavoro

Premi `r` prima del lavoro e di nuovo al termine: vengono conservati una sessione
SQLite e, quando disponibile, un CSV. Con `F2` scegli e rinomina la sessione.
`Invio` apre il replay; `[` e `]` scorrono i campioni, `l` torna live.
Il replay dichiara esplicitamente i dati archiviati e non ricostruisce API live assenti.

Per un confronto, in `F2` imposta il riferimento con `a` e la candidata con `b`,
poi premi `c` o `F3`. Il verso è **B − A**. Le statistiche usano tutti i campioni;
il replay e i grafici possono usare anteprime ridotte, distribuite sull'intera durata.
Media, P95, dati mancanti e metodi dei benchmark sono spiegati in
[Sessioni e confronti](docs/SESSIONI.md).

![Confronto RUST-CPU con serie e benchmark sintetici](docs/screenshots/confronto.png)

## Comandi principali

| Tasto | Azione |
|---|---|
| `Tab` / `Shift+Tab`, `←` / `→` | Percorre tutte le tredici viste |
| `1`…`9`, `0` | Seleziona le dieci viste hardware/LLM iniziali |
| `F2`, `F3`, `F4` | Sessioni, confronto, impostazioni |
| `↑` / `↓`, `PgUp` / `PgDn`, `Home` / `End` | Selezione o scorrimento nel contesto corrente |
| `/`, `s`, `a` nei Processi | Cerca, cambia colonna, inverte ordinamento |
| `Invio` nei Processi | Dettaglio fissato al campione selezionato |
| `Spazio`, `t` | Pausa della vista, cambio tema |
| `r`, `e`, `F6` | Registrazione, esportazione contestuale, backup SQLite |
| `+` / `-` | Intervallo; nei campi Capacity Lab/Impostazioni modifica il valore |
| `[` / `]` | Campo Capacity Lab; altrimenti campione del replay |
| `b` / `Shift+B` nelle LLM | Benchmark singolo / ripetuto, con conferma |
| `x` / `Shift+X` nella CPU | Microbenchmark singolo / ripetuto, con conferma |
| `c` durante un benchmark | Richiede annullamento |
| `F5` | Aggiorna runtime e cronologia |
| `?` / `F1` | Guida; `Esc` chiude il pannello o torna dal replay |
| `q`, `Ctrl+C` | Esce dall'applicazione; `q` funziona anche nella guida |

Nelle Sessioni `a` e `b` scelgono A/B, `n` rinomina e `Canc` chiede conferma per
l'eliminazione. Nelle Impostazioni `↑↓` sceglie il campo e `+/-` lo modifica.
La pausa congela soltanto la vista: raccolta e registrazione continuano.

## Runtime, credenziali e observer

Impostazioni iniziali: Ollama `127.0.0.1:11434`, LM Studio `127.0.0.1:1234`,
llama.cpp `127.0.0.1:8080`. Gli URL sono modificabili con `--ollama-url`,
`--lm-studio-url`, `--llama-cpp-url` e vengono ricordati. Sono accettate origini
HTTP loopback, senza credenziali nell'URL; i proxy di ambiente e i redirect non vengono usati.
Un runtime offline non blocca il monitor hardware. `--no-llm` disattiva le sonde automatiche.

Per runtime protetti sono supportate `RUST_CPU_OLLAMA_TOKEN`,
`RUST_CPU_LM_STUDIO_TOKEN` e `RUST_CPU_LLAMA_CPP_TOKEN`. I valori restano nelle
variabili d'ambiente del processo e non vengono salvati fra preferenze e report.
Il proxy ha una credenziale distinta, `RUST_CPU_OBSERVER_TOKEN`; se l'upstream è
protetto, richiede anche questa protezione all'ingresso. Il token del client
observer non viene inoltrato al runtime.

```powershell
# Benchmark esplicito su un modello generativo già installato: tre prove
.\rust-cpu.exe --benchmark-runtime ollama --benchmark-model MODELLO_INSTALLATO --benchmark-runs 3

# Proxy opzionale, in un secondo terminale e con lo stesso database della TUI
.\rust-cpu.exe --database .\archivio.sqlite3 --observe-runtime ollama --observe-port 18434
```

Configurare il client esterno per usare il proxy: le richieste inviate direttamente
al runtime non vengono osservate. `--observe-session ID` associa le misure a una
sessione aperta. Non si archiviano prompt o risposte; il runtime e il client possono
avere proprie politiche di registrazione. Configurazione, credenziali, rotte e limiti:
[Observer](docs/OBSERVER.md).

## Precisione e limiti

- **Hardware:** niente driver kernel installati automaticamente, SPD diretto,
  timing RAM live, SMBus/MSR o SMART. Temperature/tensioni aggiuntive possono
  essere lette da LibreHardwareMonitor soltanto se già disponibile e attivo.
- **GPU:** WDDM/DXGI supporta i vendor Windows quando il driver espone i contatori.
  NVIDIA e AMD e le attribuzioni per PID sono state provate sul sistema di sviluppo;
  questo non certifica ogni GPU Intel, driver o configurazione multi-GPU. La RAM
  condivisa rimane distinta dalla dedicata e le attribuzioni dei PID non si sommano
  per ricavare memoria fisica.
- **LLM:** il nome del processo è un indizio; non attribuisce automaticamente VRAM
  e lavoro a una conversazione. L'observer misura soltanto richieste instradate
  attraverso di esso. `native_decode` ed `end_to_end` hanno significati diversi.
- **Stime e benchmark:** Capacity Lab è orientativo; CPU mix64 non è il punteggio
  CPU-Z. Un carico inferiore non dimostra da solo una maggiore velocità.

Le prove distinguono test con server HTTP simulati e verifiche native. Il supporto
auth è coperto da test HTTP, senza dichiarare una prova live su ogni runtime protetto.
LHM live e piattaforme/hardware non presenti nelle prove restano non verificati:
[Hardware](docs/HARDWARE.md), [Verifiche](docs/VERIFICHE.md).

## Sviluppo, distribuzione e dati personali

Per compilare servono Rust **1.95 o superiore** e, su Windows, strumenti MSVC/Windows SDK.
Le dipendenze sono bloccate in `Cargo.lock`; SQLite è compilato con il progetto.

```powershell
cargo build --release --locked
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
.\scripts\verify.ps1 -Release

# Richiede NSIS 3.x e PowerShell 7
.\scripts\package.ps1
.\scripts\test-installer.ps1
```

La CI è configurata per Windows/Linux; i provider WDDM/DXGI/WMI e l'installer sono
Windows. Le istruzioni e la distinzione fra compilazione e prova fisica sono in
[BUILD.md](docs/BUILD.md) e [VERIFICHE.md](docs/VERIFICHE.md).

Non sono previsti account, analytics, upload dei report o pubblicazione automatica.
SQLite, log, esportazioni e report di prova locali sono esclusi da Git; la galleria
pubblica usa esclusivamente fixture sintetiche. Gli export possono comunque contenere
nomi di processi, modelli, dischi e interfacce: controllarli prima di condividerli.

Documentazione: [Manuale](docs/MANUALE.md) · [Sessioni](docs/SESSIONI.md) ·
[Hardware](docs/HARDWARE.md) · [Observer](docs/OBSERVER.md) ·
[Build e release](docs/BUILD.md) · [Verifiche](docs/VERIFICHE.md) ·
[Architettura](docs/ARCHITETTURA.md) · [Fonti](docs/FONTI.md) · [Contributi](CONTRIBUTING.md).

Licenza [MIT](LICENSE). Autore: **Daniele Gabrovec**.
