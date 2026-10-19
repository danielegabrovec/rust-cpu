# Manuale d'uso — RUST-CPU 0.2

RUST-CPU unisce monitor hardware, diagnostica delle risorse e un archivio locale di
sessioni e benchmark. Questo manuale segue il lavoro quotidiano; formule, formati e
limiti statistici sono approfonditi in [SESSIONI.md](SESSIONI.md).

## Primo avvio

Installare la versione Windows x64 oppure estrarre lo ZIP portable. Avviare RUST-CPU
dal menu Start o, da PowerShell nella cartella estratta:

```powershell
.\rust-cpu.exe
```

Usare un terminale moderno con font monospaziato Unicode. Da 100×32 celle la vista
offre più spazio; 64×18 usa una disposizione compatta. La barra laterale compare
quando il terminale è sufficientemente ampio. Ridimensionare non interrompe la raccolta.

Attendere il primo campione e l'inventario: WMI e contatori GPU possono richiedere
alcuni secondi. Un runtime LLM offline non impedisce il monitoraggio del computer.
I dati non disponibili sono indicati con `n/d`; i valori iniziali non vanno scambiati
per una misura completa. `?` o `F1` apre la guida.

SQLite è incorporato nell'eseguibile: non occorre installare un database separato.
Al primo avvio viene creato l'archivio dell'utente. La compilazione da sorgente e il
pacchetto sono descritti in [BUILD.md](BUILD.md).

## Orientarsi fra le tredici viste

`Tab` e `Shift+Tab`, oppure `←` e `→`, percorrono tutte le viste. I numeri `1`…`9`
e `0` selezionano le prime dieci; `F2`, `F3` e `F4` aprono le tre viste dell'archivio.
Nei terminali compatti le linguette cambiano gruppo per mantenere visibile la pagina attiva.

| Vista | Cosa cercare |
|---|---|
| 1 Overview | CPU, RAM, GPU, thread logici e andamento recente |
| 2 CPU | Identità, cache, istruzioni, frequenza, sensori esposti e benchmark |
| 3 Mainboard | Identità della scheda madre e firmware |
| 4 Memoria | RAM, swap e moduli dichiarati dal firmware |
| 5 GPU | Adattatori, provider, dedicata/condivisa e sensori disponibili |
| 6 Dischi / Rete | Spazio disponibile, I/O e traffico delle interfacce |
| 7 Processi | Filtro, ordinamento, CPU/RSS/I/O e attribuzioni GPU per PID |
| 8 LLM locali | Stato server, modelli, prove e cronologia delle osservazioni |
| 9 Capacity Lab | Pianificazione di pesi, KV cache e memoria disponibile |
| 0 Diagnostica | Avvisi dei collettori, fonti dei dati e limiti |
| F2 Sessioni | Registrazioni salvate, nomi, riepiloghi, A/B e replay |
| F3 Confronto | Statistiche e benchmark affiancati |
| F4 Impostazioni | Preferenze, soglie, ripetizioni e avvisi locali |

`↑↓`, `PgUp/PgDn`, `Home/End` selezionano o scorrono il contenuto. Le lunghe
caratteristiche CPU/DIMM vanno a capo e rimangono raggiungibili con lo scorrimento.
Sui terminali ampi il clic sulla sidebar cambia vista; la rotella scorre. La navigazione
da tastiera non richiede il mouse.

`q` esce dalla vista ordinaria e anche dalla guida; `Ctrl+C` richiede l'uscita.
`Esc` prima chiude un filtro o pannello, oppure torna dal replay alla vista live;
nella vista ordinaria permette di uscire. Nei pannelli di conferma `Esc` annulla
l'operazione, senza eseguirla.

## Leggere correttamente il monitor hardware

La percentuale CPU di un processo è normalizzata sull'intera capacità logica del
computer. Un thread occupato non significa necessariamente 100% della macchina.
La RAM del processo è memoria residente; la somma di più processi può includere
pagine condivise. La frequenza può essere quella dichiarata dal sistema, diversa
dal clock effettivo istantaneo di ogni core.

Su Windows WDDM/DXGI fornisce, quando disponibili, carico e memoria GPU anche per
NVIDIA, AMD e Intel. Il carico è quello del motore più occupato, non la somma di tutti
i motori. Per ciascun PID sono separate memoria **dedicata**, **condivisa** e carico
GPU. Le attribuzioni dei processi non devono essere sommate per ricavare il consumo
fisico dell'adattatore; sulle GPU integrate parte della memoria dedicata può essere RAM riservata.

NVIDIA può aggiungere temperatura e potenza tramite `nvidia-smi` già disponibile.
WDDM e NVIDIA hanno cadenze proprie: un nuovo frame non implica una nuova lettura
del driver. Temperature e tensioni CPU aggiuntive possono provenire da LibreHardwareMonitor
solo se è già in esecuzione e pubblica i sensori WMI. RUST-CPU non lo installa o avvia.

SPD diretto, timing RAM live, SMBus/MSR, SMART e sensori proprietari non esposti non
vengono simulati. Non vengono aggiunti driver kernel. Dettagli delle fonti, differenze
fra lettura live e dichiarazione firmware e requisiti dei contatori:
[HARDWARE.md](HARDWARE.md).

Nel sistema di prova sono state ottenute letture WDDM su AMD e NVIDIA, comprese
attribuzioni per PID. Questo non certifica ogni GPU/driver Intel o tutti i sensori;
LHM live non è stato esercitato su quella configurazione. Le prove fisiche e i limiti
rimangono separati dai test dei parser in [VERIFICHE.md](VERIFICHE.md).

## Osservare un lavoro LLM

1. Avviare Ollama, LM Studio o llama.cpp e scegliere un modello generativo nel runtime.
2. Aprire **LLM locali** e verificare server e modello. `F5` richiede una nuova sonda;
   le sonde automatiche proseguono normalmente ogni circa cinque secondi.
3. Nei **Processi**, `/` cerca nome o PID. `s` cambia colonna, `a` inverte il verso.
   `Invio` apre un dettaglio fissato al campione scelto, che non cambia processo
   mentre l'elenco live si riordina.
4. Premere `r`, svolgere il lavoro e premere nuovamente `r` al termine.
5. Aprire `F2` per rinominare, rileggere o esportare la sessione.

I processi marcati LLM sono riconosciuti dal nome: il programma non legge prompt,
righe di comando o file dei pesi per attribuire una conversazione. Il carico del
computer e il carico di un processo non sono automaticamente il costo di un singolo modello.

`Spazio` congela la vista hardware: raccolta, sonde e registrazione continuano.
`+` aumenta l'intervallo e riduce la frequenza dei campioni; `-` fa l'opposto.
L'intervallo è 500–10000 ms. `sampling_ms` negli export misura il **costo della raccolta**,
non questo intervallo.

### Runtime e autenticazione

Gli endpoint iniziali sono Ollama `http://127.0.0.1:11434`, LM Studio
`http://127.0.0.1:1234` e llama.cpp `http://127.0.0.1:8080`. Le opzioni `--ollama-url`,
`--lm-studio-url` e `--llama-cpp-url` scelgono origini HTTP loopback e vengono salvate
nelle preferenze. Non inserire token nell'URL. Endpoint remoti, redirect e proxy HTTP
di ambiente non sono accettati dal client RUST-CPU. Un server locale può comunque
avere un proprio instradamento remoto: verificarne la configurazione.

`--no-llm` disattiva le sonde automatiche dei runtime. Non interrompe le applicazioni
LLM già attive e non elimina i contatori hardware dei loro processi.

| Server protetto | Variabile d'ambiente letta da RUST-CPU |
|---|---|
| Ollama | `RUST_CPU_OLLAMA_TOKEN` |
| LM Studio | `RUST_CPU_LM_STUDIO_TOKEN` |
| llama.cpp | `RUST_CPU_LLAMA_CPP_TOKEN` |
| Proxy observer | `RUST_CPU_OBSERVER_TOKEN`, distinta dal token upstream |

Le variabili devono essere disponibili nel terminale che avvia il programma. I token
non vengono inseriti nel database delle preferenze o nei report. Istruzioni con
immissione protetta e separazione fra credenziale del client e del runtime:
[OBSERVER.md](OBSERVER.md#credenziali-del-client-del-proxy-e-del-runtime).
Il supporto auth è verificato da test HTTP locali simulati; non viene presentato come
prova live di tutte le configurazioni protette.

### Richieste di altre applicazioni: observer facoltativo

Il proxy si avvia esplicitamente in un secondo terminale, usando lo stesso database
della TUI:

```powershell
.\rust-cpu.exe --database .\archivio.sqlite3 --observe-runtime ollama --observe-port 18434
```

Configurare il client esterno per inviare richieste a `127.0.0.1:18434`: per Ollama
si usano le rotte native; per i client OpenAI verso LM Studio/llama.cpp la base URL
termina con `/v1`. Il proxy osserva soltanto le richieste che lo attraversano.
Non intercetta altre applicazioni, non scarica modelli e non è un sostituto completo
delle API amministrative dei runtime.

Per associare le misure a una registrazione delle risorse, aggiungere
`--observe-session ID` con l'ID di una sessione **aperta** nello stesso archivio.
Senza associazione le osservazioni restano nella cronologia globale. Se il runtime
upstream richiede un token, configurare anche il token observer: l'accesso al proxy
non deve rimuovere la protezione del server.

Modello, contatori e tempi possono essere conservati; prompt e risposte non vengono
persistiti dall'observer. Senza contatori forniti dal runtime, token e velocità restano
`n/d`: non vengono stimati contando caratteri. Le metriche native di decode e quelle
calcolate sulla durata complessiva restano distinte. Rotte, timeout, limiti e arresto:
[OBSERVER.md](OBSERVER.md).

## Sessioni, replay e confronto

`r` avvia una sessione SQLite e tenta anche un CSV; un secondo `r` ferma la sessione.
Attendere il primo campione valido prima di registrare. La barra di stato distingue
caricamento, salvataggio ed eventuali errori. Un benchmark volontario senza una
registrazione attiva crea una sessione automatica; durante una registrazione ne usa
quella già aperta.

In `F2`:

- `Invio` carica il replay; `n`, poi `Invio`, rinomina; `Esc` annulla la modifica.
- `a` sceglie il riferimento A, `b` la candidata B; `c` carica il confronto.
- `Canc` chiede conferma per eliminare campioni ed eventi della sessione. La sessione
  in registrazione non può essere eliminata.
- `e` esporta la sessione selezionata; `F6` richiede un backup dell'intero database.

Il badge **REPLAY** rende esplicito che si stanno leggendo dati archiviati.
`[` e `]` cambiano campione; `l` torna live. Nella pagina Capacity Lab `[` e `]`
rimangono dedicati ai suoi campi: spostarsi su un'altra pagina per navigare il replay.
La lista dei processi archiviati contiene solo quelli riconosciuti come LLM e non
costituisce l'elenco completo del sistema operativo. Gli stati API live dei runtime
non vengono ricostruiti; benchmark e osservazioni salvati rimangono consultabili.

Il replay usa al massimo 2.000 campioni distribuiti sull'intera sessione. Le statistiche
di confronto usano invece tutti i campioni salvati; i grafici del confronto hanno
un'anteprima di massimo 400 punti per sessione e avanzamento temporale normalizzato.
Il numero di punti mostrati non è il numero totale di osservazioni conservate.

`F3` confronta **B − A**. Scorrere per leggere gruppi di benchmark, grafici, avvisi
e provenienza. `e` esporta il confronto. A uguale a zero rende `Δ%` non definito,
quindi `n/d`; dati assenti non diventano zero. Una media dei campioni non è una media
pesata per il tempo e un carico inferiore non dimostra un lavoro completato più rapidamente.

Formule, P95, nomi/stati, normalizzazione, gruppi compatibili e limiti di import/export:
[SESSIONI.md](SESSIONI.md).

## Benchmark volontari

**LLM:** nella pagina LLM selezionare il modello e premere `b`, leggere la conferma e
premere `Invio`. `Shift+B` richiede una serie di prove secondo le preferenze. Ogni
richiesta usa un prompt sintetico e chiede al massimo 64 token di output. Può caricare
il modello e consumare CPU, RAM e GPU. I soli modelli di embedding non sono candidati
per questa prova; un elenco API compatibile non certifica ogni capacità generativa.

**CPU:** `x` chiede una prova, `Shift+X` una serie. Il kernel intero `mix64 v1` usa una
fase single thread e una multi thread, ciascuna di circa 600 ms, con massimo 16 worker.
La conferma mostra il numero di prove e il carico previsto. Il risultato è in mix al
secondo: non è il punteggio CPU-Z, non misura FLOPS e non prevede prestazioni LLM.
Confrontare lo stesso kernel e profilo di build; una build debug può essere molto più lenta.

Il numero di ripetizioni è impostabile da `F4` fra 1 e 10. `Esc` annulla una conferma;
`c` richiede l'annullamento di un benchmark in corso. La cancellazione della richiesta
non certifica che il server LLM abbia già fermato il calcolo.

Per le LLM, TTFT comprende attesa, caricamento, prefill e trasporto fino al primo
contenuto ricevuto; non è il solo tempo di calcolo. `native_decode` ed `end_to_end`
misurano durate diverse. I gruppi di confronto non mescolano automaticamente i metodi;
contesto, quantizzazione, cache, offload e carico di fondo rimangono condizioni da controllare.

Nella pagina LLM `PgUp/PgDn` scorre risultati e cronologia SQLite, mentre `↑↓` seleziona
il modello. La cronologia distingue benchmark CPU, benchmark LLM e richieste osservate,
compresi eventi senza sessione. Una richiesta esterna osservata non diventa un test
sintetico controllato soltanto perché usa lo stesso modello.

## Preferenze e avvisi

`F4` apre nove campi: tema, intervallo, avvisi attivi, soglie CPU/RAM/VRAM,
temperature GPU/CPU e ripetizioni. `↑↓` sceglie il campo e `+/-` lo modifica.
`PgUp/PgDn` scorre informazioni del database e lista degli avvisi.

Il salvataggio è automatico. Vengono ricordati anche ordinamento dei processi,
direzione, parametri Capacity Lab e URL locali dei runtime. `t` cambia il tema
direttamente dalle altre pagine. Gli override CLI dell'intervallo o degli URL
aggiornano la preferenza salvata. `--reset-settings` ripristina le impostazioni
senza cancellare le sessioni.

Gli avvisi sono eventi locali al superamento della soglia, con cooldown per sensore
(predefinito 30 secondi). Non sono notifiche del sistema operativo. Disattivarli
sospende i nuovi eventi e conserva lo storico. Un sensore mancante non equivale a
un valore sotto soglia: impostare una soglia di temperatura non crea un sensore.

## Capacity Lab

`[` e `]` scelgono uno dei sei campi; `+` e `-` lo modificano: parametri in miliardi,
bit medi per peso, contesto, layer, teste KV e dimensione testa. Le impostazioni
rimangono conservate fra gli avvii.

Il default è un modello ipotetico 8B, 4,5 bit per peso, 32 layer, GQA con 8 teste KV,
dimensione testa 128 e contesto 8192. Non è il rilevamento automatico del modello scelto.

```text
pesi_byte = parametri_miliardi × 1e9 × bit_per_peso / 8
kv_byte = 2 × layer × contesto × teste_KV × dimensione_testa × 2 byte
totale_stimato = pesi_byte × 1,15 + kv_byte + 512 MiB
```

La cache è ipotizzata FP16 e batch 1. RAM e memoria GPU dedicata sono vincoli separati;
il programma non le somma come capacità unica e non somma automaticamente più GPU.
MoE, cache quantizzata, architetture particolari, batch, memory mapping e offload
parziale possono cambiare l'uso reale. La stima non garantisce il caricamento né token/s.

## CLI e dati locali

Il database normale è `%LOCALAPPDATA%\RUST-CPU\workspace.sqlite3`. La variabile
`RUST_CPU_DATABASE` può cambiarlo; `--database` ha precedenza. Gli export interattivi
sono normalmente in `%USERPROFILE%\Documents\RUST-CPU\exports`: nelle cartelle Documenti
reindirizzate scegliere un percorso esplicito con `--export-dir`.

Aprire PowerShell nella cartella portable. Gli ID `1` e `2` seguenti sono esempi:
sostituirli con quelli restituiti dal proprio elenco. Usare nomi nuovi per i file.

```powershell
# Sessione senza TUI; --no-llm non interroga le API dei runtime.
.\rust-cpu.exe --database .\archivio.sqlite3 --save-session "baseline" --samples 3 --no-llm
.\rust-cpu.exe --database .\archivio.sqlite3 --sessions

# Una sessione completa esportata, oppure un campione della sua anteprima.
.\rust-cpu.exe --database .\archivio.sqlite3 --load-session 1 --export-session .\sessione-1.json
.\rust-cpu.exe --database .\archivio.sqlite3 --load-session 1 --sample-index 0 --tab 1 --render .\replay-1.svg

# Confronto salvato e rendering SVG, senza raccogliere nuovi campioni hardware.
.\rust-cpu.exe --database .\archivio.sqlite3 --compare 1 2 --comparison-output .\confronto-1-2.json --render .\confronto-1-2.svg

# Import, backup e controllo del database.
.\rust-cpu.exe --database .\altro-archivio.sqlite3 --import-session .\sessione-1.json
.\rust-cpu.exe --database .\archivio.sqlite3 --backup-db .\backup-001.sqlite3
.\rust-cpu.exe --database .\archivio.sqlite3 --db-check

# Prove esplicite: sostituire MODELLO_INSTALLATO con un modello generativo locale.
.\rust-cpu.exe --database .\archivio.sqlite3 --cpu-benchmark --benchmark-runs 3 --no-llm
.\rust-cpu.exe --database .\archivio.sqlite3 --benchmark-runtime ollama --benchmark-model MODELLO_INSTALLATO --benchmark-runs 3

# Preferenze iniziali, mantenendo le sessioni.
.\rust-cpu.exe --database .\archivio.sqlite3 --reset-settings
.\rust-cpu.exe --help
```

`--render` scrive SVG quando l'estensione è `.svg`, altrimenti testo UTF-8.
`--sample-index` conta da zero nei campioni caricati dell'anteprima. Le operazioni
archivio non si combinano nello stesso comando con una nuova registrazione o benchmark.
Per CSV e sessione SQLite insieme usare `--record FILE --samples N`; per uno snapshot
hardware usare `--snapshot FILE`. L'elenco completo delle opzioni è in `--help`.

I file esistenti non vengono sovrascritti. I report non vengono pubblicati su GitHub.
Database, esportazioni, log e report hardware locali sono esclusi dal repository;
le immagini pubbliche contengono fixture sintetiche dichiarate.

`F6` o `--backup-db` usa il backup SQLite e ne conferma la destinazione. Un export di
sessione conserva tutti i campioni salvati, senza il limite di 2.000 dell'anteprima;
un backup conserva l'intero archivio e le preferenze. Limiti degli eventi esportati,
dimensioni accettate dall'import e recupero sono dettagliati in [SESSIONI.md](SESSIONI.md).

## Risoluzione dei problemi

| Sintomo | Intervento |
|---|---|
| «Serve un terminale» | Aprire Windows Terminal; per script usare snapshot, registrazione o operazioni archivio. |
| Caratteri illeggibili | Usare un font monospaziato Unicode e un terminale moderno; provare `t`. |
| Dettagli fuori vista | Scorrere con frecce/PgUp/PgDn; allargare il terminale. I dettagli CPU/DIMM vanno a capo. |
| CPU/RAM inizialmente incomplete | Attendere primo campione e inventario prima di avviare una registrazione. |
| GPU senza carico o memoria | Attendere due letture; controllare provider e avvisi in Diagnostica. WDDM dipende dal driver, non solo dal vendor. |
| Temperatura/potenza GPU n/d | Queste metriche non sono garantite da WDDM; NVIDIA può integrarle con `nvidia-smi`. |
| Tensioni/temperature CPU n/d | Occorre un sensore esposto; LHM è letto solo se già attivo. Non vengono aggiunti driver. |
| Runtime offline | Verificare server/porta e premere F5; controllare l'URL salvato. |
| Runtime auth | Configurare il token della variabile corretta nel processo che avvia RUST-CPU; vedere OBSERVER.md. |
| Proxy rifiuta l'avvio con upstream protetto | Configurare anche `RUST_CPU_OBSERVER_TOKEN`, distinto dalla credenziale upstream. |
| Richiesta esterna assente dalla cronologia | Il client deve usare il proxy, la rotta supportata e lo stesso database; F5 aggiorna lo storico. |
| Modello presente ma residenza n/d | Un elenco API compatibile non dimostra che il modello sia caricato. |
| Token/s n/d | Mancano contatori/tempi del runtime; con stream OpenAI verificare il supporto di `include_usage`. |
| Confronto senza delta | Verificare dati disponibili, A diverso da zero e presenza del gruppo in entrambe le sessioni. |
| Sessione aperta/incompleta | È leggibile, ma non certifica un lavoro terminato; leggere gli errori prima di confrontarla. |
| File esistente | Scegliere un nuovo nome: la protezione da sovrascrittura è intenzionale. |
| Database occupato o backup non riuscito | Leggere il messaggio, chiudere eventuali altri strumenti di scrittura e riprovare; non assumere completamento. |
| Benchmark annullato | La cancellazione del client non garantisce l'arresto del calcolo già ricevuto dal server. |

Il comportamento implementato e l'esito dei test sono cose distinte. Le prove native
su Windows, i test HTTP con server simulati e i casi non esercitati fisicamente sono
riportati in [VERIFICHE.md](VERIFICHE.md); istruzioni di compilazione e installer in
[BUILD.md](BUILD.md).

## Disinstallazione

Usare **Disinstalla** dal menu Start o Impostazioni → App. Per la versione portable
rimuovere la cartella estratta quando il programma è chiuso. Database ed esportazioni
personali rimangono conservati nelle rispettive cartelle. Non vengono modificati
PATH, firewall, servizi o impostazioni BIOS. L'installer non è firmato digitalmente.
