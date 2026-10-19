# Compilazione, pacchetti e release

Questa guida descrive il codice e gli script presenti nel repository. Le prove
effettivamente eseguite e i relativi limiti sono registrati in
[VERIFICHE.md](VERIFICHE.md); la presenza di un workflow non certifica da sola che
la sua esecuzione su GitHub sia riuscita.

## Prerequisiti

- Rust con toolchain stable compatibile con `rust-version` in `Cargo.toml`
  (attualmente **1.95** o superiore), Cargo, rustfmt e Clippy.
- Windows x64: toolchain Rust MSVC, Visual Studio Build Tools con strumenti C++
  e Windows SDK.
- Creazione dell'installer: NSIS 3 con `makensis.exe` e le intestazioni standard
  `MUI2.nsh` e `x64.nsh`.
- Script di verifica e packaging: PowerShell 7. Il runtime CRT MSVC è collegato
  staticamente per il target Windows x64 tramite `.cargo/config.toml`.
- Linux: toolchain Rust e linker/toolchain C del sistema. L'installer NSIS e lo
  script PowerShell di packaging qui descritto producono pacchetti Windows.

Le dipendenze Rust sono registrate in `Cargo.lock`. Il programma non richiede
Node.js, un servizio cloud o credenziali per compilarsi. Un server LLM locale e
il driver NVIDIA sono funzionalità opzionali, indipendenti dalla compilazione.

## Compilazione da checkout

Eseguire dalla cartella che contiene `Cargo.toml`:

```powershell
rustc --version
cargo --version
cargo build --release --locked
```

Su Windows il binario risultante è `target\release\rust-cpu.exe`:

```powershell
.\target\release\rust-cpu.exe --help
.\target\release\rust-cpu.exe --snapshot .\test-output\manuale.json --no-llm
.\target\release\rust-cpu.exe
```

L'avvio interattivo richiede stdin e stdout collegati a un terminale. La modalità
`--snapshot` funziona senza TUI; il percorso di destinazione deve essere nuovo.
Usare un nome diverso a ogni ripetizione: gli export proteggono i file esistenti.

Su Linux l'eseguibile si trova in `target/release/rust-cpu`:

```sh
cargo build --release --locked
./target/release/rust-cpu --snapshot ./test-output/manuale.json --no-llm
./target/release/rust-cpu
```

## Controlli prima del pacchetto

```powershell
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
.\scripts\verify.ps1 -Release
```

`cargo test` comprende test brevi del kernel CPU e mock locali per i protocolli.
Il benchmark CPU completo richiede una richiesta esplicita. La riuscita dei test
non prova disponibilità di sensori fisici, accuratezza dei firmware, esecuzione
su altre architetture o correttezza di un runtime LLM reale non avviato.

Il workflow `.github/workflows/ci.yml` configura una matrice Windows/Linux con
formattazione, Clippy, test, build release e snapshot senza LLM. Il job Windows
installa NSIS con Chocolatey quando assente, poi richiama lo script di packaging
e carica gli artefatti di quel job. Consultare l'esito della singola
esecuzione prima di dichiarare verificata la CI.

## ZIP portable e installer Windows

Dalla radice del progetto:

```powershell
.\scripts\package.ps1
```

Se NSIS non viene trovato automaticamente, indicare il file esatto:

```powershell
.\scripts\package.ps1 -MakeNsis 'C:\Program Files (x86)\NSIS\makensis.exe'
```

Lo script `scripts/package.ps1`:

1. Esegue `cargo build --release --locked` e interrompe il pacchetto se fallisce.
2. Legge la versione tramite `cargo metadata` dal manifest Cargo.
3. Prepara in `artifacts` una cartella con eseguibile, licenza, README, changelog
   e documentazione e genera lo ZIP portable.
4. Richiama `installer/rust-cpu.nsi` passando `/DVERSION=<versione>` a NSIS.
5. Scrive `artifacts/SHA256SUMS.txt` con gli hash SHA-256 dello ZIP e del setup.

Per la versione `0.1.0` i file prodotti si chiamano:

```text
artifacts/rust-cpu-0.1.0-windows-x64-portable.zip
artifacts/rust-cpu-0.1.0-windows-x64-setup.exe
artifacts/SHA256SUMS.txt
```

La ricerca automatica di NSIS considera `makensis` nel PATH e i percorsi indicati
nel sorgente dello script, compresa una possibile installazione locale Tauri.
Lo script non installa NSIS né scarica il compilatore.

Il suffisso `windows-x64` esprime il target del pacchetto distribuito. Per
produrlo usare una toolchain host Windows x64 MSVC e controllare `rustc -vV`:
lo script usa la build host e non imposta autonomamente `--target`.

## Comportamento dell'installer

Il sorgente NSIS richiede Windows x64 e usa `RequestExecutionLevel user`:
l'installazione è per l'utente corrente, con destinazione predefinita
`%LOCALAPPDATA%\Programs\RUST-CPU`. Registra installazione e disinstallazione in
HKCU e crea collegamenti nel menu Start dell'utente.

Il setup copia il binario e la documentazione inclusa nello script, poi crea
`Uninstall.exe`. Non configura un servizio, un'attività pianificata o una voce
di avvio automatico. Non aggiunge autonomamente il binario al PATH.

La disinstallazione rimuove i file distribuiti, i collegamenti e le chiavi
registrate. Gli export dell'utente si trovano fuori dalla cartella del programma
e non vengono eliminati dallo script di disinstallazione.

Per un test isolato, scegliere una cartella di installazione dedicata, avviare
il binario installato con `--version` e uno snapshot nuovo, quindi eseguire il
disinstallatore e controllare il risultato. Un test silent può usare `/S`; il
percorso `/D=...` di NSIS deve essere l'ultimo argomento. Registrare il percorso
e i comandi realmente verificati in `VERIFICHE.md`.

## Versione e pubblicazione

La versione dell'applicazione e degli export viene compilata da `Cargo.toml`
tramite `CARGO_PKG_VERSION`. Il nome del setup e le stringhe di versione NSIS
ricevono `VERSION` dallo script di packaging. Prima di una release verificare
anche la versione numerica Windows `VIProductVersion` nel sorgente NSIS: il
formato richiesto è composto da quattro numeri, per esempio `0.1.0.0`.

Prima di caricare una release:

- Allineare manifest, changelog, tag Git e versione del setup.
- Usare un pacchetto generato dall'esatto commit che si intende pubblicare.
- Verificare il contenuto dello ZIP e il ciclo installazione/disinstallazione.
- Controllare gli hash dei file definitivi, dopo qualsiasi eventuale firma.
- Allegare setup, ZIP e `SHA256SUMS.txt` alla release corrispondente.

I comandi seguenti sono un esempio manuale per una release già revisionata;
richiedono GitHub CLI autenticata e il remote Git corretto:

```powershell
git tag v0.1.0
git push origin v0.1.0
gh release create v0.1.0 `
  .\artifacts\rust-cpu-0.1.0-windows-x64-setup.exe `
  .\artifacts\rust-cpu-0.1.0-windows-x64-portable.zip `
  .\artifacts\SHA256SUMS.txt `
  --verify-tag --title 'RUST-CPU 0.1.0' --notes-file .\CHANGELOG.md
```

La disponibilità di un hash non equivale a una firma del produttore. Il codice
di packaging non applica una firma Authenticode: non presentare un setup come
firmato senza una verifica separata del file finale.

## Portabilità e limiti

| Componente | Windows x64 | Linux |
| --- | --- | --- |
| TUI, snapshot JSON, registrazione CSV | Implementati | Implementati tramite le API portabili |
| Carico CPU/RAM/processi/dischi/rete | Provider sysinfo | Provider sysinfo; disponibilità dipendente dal sistema |
| CPUID dettagliato | x86/x86-64 | x86/x86-64 |
| Scheda madre, BIOS, moduli DIMM, GPU di inventario | WMI, con timeout iniziale | Nessun provider WMI |
| GPU NVIDIA live | `nvidia-smi` quando disponibile | `nvidia-smi` quando disponibile |
| GPU AMD/Intel live | Nessun provider dinamico dedicato | Nessun provider dinamico dedicato |
| Installer distribuito | NSIS per utente x64 | Nessun pacchetto Linux creato da NSIS |

macOS, Windows ARM64, target Linux non x86 e hardware con gruppi processori
particolari richiedono verifiche dedicate prima di dichiarare supporto provato.
La compilazione condizionale evita WMI fuori da Windows e CPUID fuori da x86;
questo non sostituisce un test sull'hardware corrispondente.

Clock OS nominali, sensori non esposti, contatori non accessibili e assenza di
`nvidia-smi` producono limiti dichiarati o dati mancanti. Il programma non include
un driver kernel per SPD, timing RAM, tensioni, SMART o sensori proprietari.
