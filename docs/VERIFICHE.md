# Verifiche — RUST-CPU 0.2.0

Data: **19 ottobre 2026**. Le prove fisiche sono state eseguite su Windows x64,
Rust/Cargo 1.99.0 MSVC. Nessun report personale o database di prova viene pubblicato;
le immagini della documentazione sono fixture sintetiche dichiarate.

## Gate 0.2

Passati formattazione, Clippy con `-D warnings`, suite unit/HTTP/SQL/rendering/CLI
e build release. 110 test automatici su Windows: la suite controlla anche transazioni,
schema estraneo/futuro, ripristino/backup, importi incoerenti, vincoli e timeout.
Gli ID, apostrofi e contenuti simili a SQL vengono passati come parametri.

| Prova reale | Esito |
|---|---|
| Sessioni SQLite | Cattura, chiusura, riapertura, rinomina e replay riusciti |
| Confronto A/B | Risorse, delta e gruppi CPU con3 prove per lato ottenuti |
| Integrità e backup | Backup riaperto:7 sessioni,10 benchmark,1 avviso; quick_check=ok |
| Preferenze | Tema chiaro e intervallo500ms salvati e letti nuovamente dal database |
| Avvisi | SogliaCPU di prova abbassata su archivio isolato: evento reale salvato; soglia poi ripristinata |
| GPU Windows | Carico e memoria dedicata/condivisa letti sia da AMD integrata sia da NVIDIA |
| GPU per PID | Attribuzioni dedicate/condivise e carico motori ottenuti su processi reali |
| Observer Ollama | Stream reale32 token indipendente e16 token associati alla sessione: contatori server e SQLite concordi |
| Benchmark ripetuti | TUICPU3 prove con sessione automatica; CLI CPU3 e LM Studio2 prove, tutte archiviate |
| Terminale | F2/F3/F4, r, selezioneA/B, rinomina, replay, ritorno live, F6backup, tema e uscita q dalla guida riusciti |
| Auth proxy | Avvio reale rifiutato senza tokenfrontend quando upstream protetto; nessuna porta rimasta aperta |

I32 casi HTTP LLM/observer verificano inoltre autenticazione Bearer distinta per
frontend/upstream, mancata propagazione delle chiavi, streaming, quote, concorrenza,
redirect, dati malformati e cancellazione. Non sono prove di un runtime personale
configurato con una chiave reale: per evitare modifiche a quel servizio, tale parte
è coperta da server locali di test e dal controllo di policy all'avvio.

Le prove benchmark parallele ad altre attività verificano il funzionamento: non
costituiscono un confronto prestazionale pubblicato. Le metriche native decode e
quelle end-to-end restano separate. L'export di un confronto visualizzato è congelato,
anche se nel frattempo arrivano altri campioni. Una regressione lo controlla.

La riproduzione usa fino a2000 punti distribuiti sull'intera sessione; i confronti
ne usano fino a400 per lato. Le statistiche SQL usano tutti i campioni. Export completo,
import JSON limitato a64MiB/50000 campioni; backup SQLite per un intero archivio.

Installer0.2 verificato in cartella isolata: avvio, due campioni in SQLite, snapshot reale, quick_check=ok, disinstallazione e archivio conservato. ZIP portable, documentazione, immagini e checksum inclusi; compilazione NSIS senza warning.

## Copertura e limiti correnti

- WDDM provato fisicamente con AMD e NVIDIA. Il percorso Intel è implementato nello
  stesso provider ma non è stato provato su hardware Intel.
- LibreHardwareMonitor WMI non era attivo: parser e assenze verificati, letture CPU
  live non provate con quel provider. Il programma non installa driver kernel.
- SPD raw e timing live richiedono un provider hardware appropriato: restano esclusi.
- llama.cpp verificato a livello di protocollo, senza server reale disponibile.
- Il proxy osserva soltanto richieste instradate volontariamente: nessuna intercettazione
  automatica delle conversazioni di altri programmi. Non salva prompt né risposte.
- La memoria perPID può contare risorse condivise; non è una misura esclusiva per conversazione.
- Installer non firmato; macOS, ARM64, installazione Windows vergine e desktopLinux fisico non certificati.

CI pubblica per Windows/Linux e singoli esiti:
[GitHub Actions](https://github.com/danielegabrovec/rust-cpu/actions/workflows/ci.yml).
La presenza del workflow non sostituisce l'esito del commit. Comandi ripetibili in
[BUILD](BUILD.md), [SESSIONI](SESSIONI.md), `scripts/verify.ps1 -Release` e `scripts/test-installer.ps1`.

---

# Archivio delle verifiche — RUST-CPU 0.1.0

Data: **19 ottobre 2026**. Prima piattaforma provata fisicamente: Windows x64,
toolchain Rust/Cargo 1.99.0 MSVC, NSIS 3.11. I report hardware locali, le sessioni
e i log rimangono esclusi da Git. Le immagini pubbliche usano dati sintetici e
sono dichiarate come anteprime, non come risultati di misurazione.

## Controlli del codice

Superati `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`,
`cargo test --all-targets --locked` e build release. La suite comprende:

- Rapporti, limiti delle percentuali, formule GQA/KV e storico limitato.
- Delta I/O, riuso dei contatori, CPU normalizzata e parser NVIDIA con assenze esplicite.
- Richieste HTTP a server loopback simulati: protocolli Ollama/LM Studio/OpenAI,
  NDJSON/SSE frammentati, Unicode, errori, auth, redirect rifiutati, limiti risposta,
  modelli remoti/encoder esclusi, contatori cache e cancellazione durante uno stream.
- Rendering di tutte le viste con dati assenti, nomi lunghi, temi, modali e dimensioni
  da terminale nullo a 160×48; navigazione End→Up, valori mancanti, identità del dettaglio processo.
- CLI avviata come processo reale: JSON, CSV e SVG insieme, file esistenti preservati,
  URL remoti rifiutati, opzioni incompatibili, help e avvio senza terminale.

La CI ripete i controlli su Windows e Linux e produce un pacchetto Windows.
Lo stato aggiornato della singola esecuzione è consultabile in
[GitHub Actions](https://github.com/danielegabrovec/rust-cpu/actions/workflows/ci.yml).
I risultati Linux di CI non equivalgono a una prova su un desktop Linux fisico.

## Dati reali e runtime

| Prova | Esito osservato |
|---|---|
| CPUID, identità CPU, core/thread, cache e istruzioni | Letture reali ottenute |
| WMI mainboard/BIOS e moduli RAM | Dati ottenuti senza amministratore |
| CPU, RAM, processi, dischi e interfacce | Campioni reali e serie CSV ottenuti |
| GPU NVIDIA | Utilizzo, VRAM, temperatura e potenza letti con nvidia-smi |
| GPU AMD integrata | Inventario presente; sensori dinamici n/d come previsto |
| Sonde Ollama e LM Studio | Server reali online; modelli ricevuti dalle API |
| Benchmark Ollama | Inferenza reale riuscita; token, decode nativo, prefill e TTFT ricevuti |
| Benchmark LM Studio | Inferenza reale riuscita; 64 token output, TTFT e throughput end-to-end |
| Benchmark CPU | Fasi single/multi completate nella release; risultati finiti e positivi |
| llama.cpp | Nessun server reale disponibile: offline; protocollo coperto da server di test |

Il benchmark Ollama è stato eseguito con un modello locale già installato di piccole
dimensioni; quello LM Studio con un modello testuale già disponibile. Non sono stati
scaricati modelli. Le misure non vengono presentate come confronto prestazionale
tra runtime, poiché modelli e condizioni erano diversi.

## Terminale reale e pacchetto

Eseguito smoke test ConPTY Windows a 80×24: apertura, caricamento dati, cambio scheda,
ricerca processo, dettaglio, esportazione, registrazione, pausa, tema, Capacity Lab,
benchmark CPU confermato, guida e uscita con ripristino del terminale. Una sessione
di 11 righe ha continuato a registrare mentre la vista era in pausa. Il report
esportato dalla TUI include il benchmark CPU terminato.

Installer e ZIP portable creati dallo script di packaging. Verificato il contenuto
dello ZIP. Il test isolato dell'installer ha controllato eseguibile, documentazione,
immagini, avvisi delle dipendenze, avvio `--version`, snapshot hardware reale,
disinstallazione e rimozione di chiavi/collegamenti del test. Le esportazioni
personali rimangono conservate. SHA-256 forniti insieme agli artefatti.

L'installer è **non firmato digitalmente**, come confermato dalla lettura Authenticode.
Non è stata simulata l'esperienza di SmartScreen su un PC nuovo. Il CRT MSVC è
collegato staticamente; non è stata provata una macchina Windows appena installata.

## Limiti ancora presenti

- Nessun driver kernel: SPD raw, timing RAM live, tensione CPU e sensori proprietari
  non sono implementati. I dati SMBIOS sono dichiarazioni firmware.
- GPU AMD/Intel: inventario, senza un provider dinamico dedicato. Multi-GPU NVIDIA
  gestita dal parser, ma non verificata fisicamente con più schede NVIDIA.
- Nessun token/s passivo delle generazioni di altre applicazioni e nessuna attribuzione
  completa della VRAM a singole conversazioni. Il CSV GPU usa il primo adattatore.
- API locali protette da token: stato auth riconosciuto, credenziali non gestite.
- Capacity Lab orientativo; CPU mix64 non equivalente al punteggio CPU-Z.
- macOS, Windows ARM64, Linux desktop fisico e altri hardware non certificati da questa prova.

Comandi ripetibili: [BUILD.md](BUILD.md), `scripts/verify.ps1 -Release` e
`scripts/test-installer.ps1`. I test di protocollo sono separati dalle prove reali
per non far dipendere la suite da modelli installati o server personali.
