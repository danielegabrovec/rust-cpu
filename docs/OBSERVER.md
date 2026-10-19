# Osservare richieste LLM locali reali

L'observer è un **proxy HTTP locale facoltativo e avviato esplicitamente**. Un client LLM invia le richieste al proxy; il proxy le inoltra al runtime locale configurato, restituisce il corpo della risposta e registra soltanto modello, tempi e contatori. Non intercetta il traffico di altre applicazioni e non legge conversazioni dai file dei runtime. Una richiesta inviata direttamente al runtime non compare nello storico osservato.

L'observer si avvia in un terminale separato dalla TUI. La TUI può leggere lo stesso archivio SQLite e aggiornare lo storico delle osservazioni. I risultati osservati sono distinti dai microbenchmark sintetici dell'applicazione.

## Avvio

In PowerShell, scegliere un archivio locale esplicito e usarlo in entrambi i terminali:

```powershell
$dbPath = Join-Path $env:USERPROFILE 'Documents\RUST-CPU\workspace.sqlite3'
rust-cpu.exe --database "$dbPath"
```

Nel secondo terminale, ripetere la definizione della variabile e avviare, per esempio, il proxy verso LM Studio:

```powershell
$dbPath = Join-Path $env:USERPROFILE 'Documents\RUST-CPU\workspace.sqlite3'
rust-cpu.exe --database "$dbPath" --observe-runtime lm-studio --observe-port 18434 --lm-studio-url http://127.0.0.1:1234
```

Alternative:

```powershell
rust-cpu.exe --database "$dbPath" --observe-runtime ollama --observe-port 18434 --ollama-url http://127.0.0.1:11434
rust-cpu.exe --database "$dbPath" --observe-runtime llama.cpp --observe-port 18434 --llama-cpp-url http://127.0.0.1:8080
```

Avviare una sola di queste alternative sulla porta 18434. Il processo ascolta esclusivamente su **127.0.0.1**. La porta del proxy deve essere diversa da quella del runtime upstream; gli URL upstream devono essere origini HTTP loopback, senza credenziali, percorsi, query o frammenti. Il programma rifiuta host remoti, proxy di ambiente e redirect HTTP.

## Configurare il client

Per un client compatibile OpenAI, impostare la base URL:

```text
http://127.0.0.1:18434/v1
```

Le chat completions attraversano così `/v1/chat/completions`. Il campo `model` deve indicare un modello generativo supportato dal runtime. LM Studio può classificare come `llm` anche componenti che non supportano questa rotta: l'elenco non è una prova di capacità generativa. Non viene inviata una richiesta di inferenza per verificare automaticamente tale capacità.

Per ottenere i contatori da un client OpenAI in streaming, quando il runtime lo supporta, impostare nella richiesta:

```json
{"stream": true, "stream_options": {"include_usage": true}}
```

Il proxy conserva la richiesta originale: non aggiunge opzioni né modifica il prompt. Se il client non chiede `usage` e il runtime non restituisce tempi/contatori nativi, la richiesta può essere osservata con durata e TTFT ma con token e tok/s n/d. Un runtime che rifiuta `include_usage` restituisce il proprio errore al client senza una misura dichiarata riuscita.

Per un client Ollama che consente di scegliere l'host:

```powershell
$env:OLLAMA_HOST = 'http://127.0.0.1:18434'
ollama list
ollama ps
```

`list` e `ps` attraversano soltanto le rotte di metadati e non generano osservazioni di inferenza. I client che usano direttamente `/api/chat` o `/api/generate` possono instradare lì le richieste. L'observer non è un sostituto completo del server Ollama: comandi che richiedono rotte aggiuntive, download, gestione dei modelli o `/api/show` non sono supportati.

Una richiesta Ollama esplicita di prova, dopo aver sostituito il nome con quello di un modello generativo già installato:

```powershell
$modelName = Read-Host 'Nome del modello generativo locale già installato'
$payload = @{ model = $modelName; prompt = 'Scrivi una breve frase sul cielo.'; stream = $false } | ConvertTo-Json
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:18434/api/generate -ContentType 'application/json' -Body $payload
```

Questa è inferenza richiesta dall'utente e può caricare il modello secondo la configurazione del runtime. L'observer non scarica modelli. Per Ollama la generazione viene inoltrata soltanto se il modello risulta installato con dimensione positiva e privo di indicatori remoti; modelli cloud e encoder embedding noti vengono bloccati prima dell'inoltro del prompt.

Per ripristinare il client Ollama alla configurazione precedente, rimuovere la variabile da quel terminale o ripristinarne il valore precedente:

```powershell
Remove-Item Env:OLLAMA_HOST -ErrorAction SilentlyContinue
```

## Collegare le osservazioni a una sessione di lavoro

Avviare una registrazione delle risorse nella TUI e annotare l'ID della sessione nell'archivio. Avviare il proxy con lo stesso database e l'ID effettivo:

```powershell
$sessionId = Read-Host 'ID della sessione presente nello stesso archivio'
rust-cpu.exe --database "$dbPath" --observe-runtime lm-studio --observe-port 18434 --observe-session "$sessionId"
```

L'associazione è esplicita: il proxy non indovina a quale progetto o sessione appartenga il traffico. Senza `--observe-session`, le osservazioni restano nello storico senza un'associazione di sessione. Il programma verifica l'ID; non usare un numero d'esempio al posto di una sessione esistente.

Mantenere nella TUI gli URL dei runtime reali e cambiare la base URL dei client esterni che si vogliono osservare. Se si instrada attraverso il proxy anche un microbenchmark RUST-CPU, quella stessa inferenza può comparire come test sintetico e come richiesta osservata: sono due registrazioni dello stesso lavoro, non due lavori indipendenti.

L'aggiornamento dello storico TUI è periodico e non modifica il traffico del client. Le risorse campionate descrivono il sistema o i processi riconosciuti: l'associazione temporale non dimostra che tutta la CPU/RAM/VRAM fosse consumata da una singola richiesta.

## Credenziali del client, del proxy e del runtime

Le credenziali vengono lette esclusivamente dalle variabili d'ambiente del processo RUST-CPU:

| Runtime | Variabile |
| --- | --- |
| Ollama | `RUST_CPU_OLLAMA_TOKEN` |
| LM Studio | `RUST_CPU_LM_STUDIO_TOKEN` |
| llama.cpp | `RUST_CPU_LLAMA_CPP_TOKEN` |
| Accesso al proxy observer | `RUST_CPU_OBSERVER_TOKEN` |

Il programma costruisce l'header Bearer del runtime corretto usando la sua variabile upstream. **Il client esterno deve conoscere soltanto il token observer**, distinto da quello del runtime. Il token observer viene verificato con confronto a tempo costante e non viene inoltrato al runtime; l'header upstream usa esclusivamente il token del runtime. I cookie del client non vengono inoltrati.

Se `RUST_CPU_OBSERVER_TOKEN` è impostato, tutte le rotte richiedono `Authorization: Bearer <token-observer>`; una chiave mancante o errata riceve HTTP 401 prima di qualsiasi inoltro. Se è configurato un token upstream ma manca quello observer, **l'avvio viene rifiutato**: il proxy non rimuove la protezione del runtime per gli altri utenti locali. Senza token upstream, il token observer è facoltativo; se nessuno dei due è configurato, il comportamento locale senza autenticazione rimane disponibile.

Non ci sono opzioni CLI per inserire il valore dei token. I valori non vengono aggiunti a report, metriche, stato UI o messaggi di errore. Il token observer deve contenere caratteri ASCII visibili senza spazi o controlli, con limite di 8192 byte; usare una chiave casuale lunga e distinta dalla credenziale upstream.

Per impostare il token LM Studio nel terminale che avvierà RUST-CPU senza digitarlo come comando salvato nella cronologia:

```powershell
$secureToken = Read-Host 'Token del runtime locale' -AsSecureString
$env:RUST_CPU_LM_STUDIO_TOKEN = [System.Net.NetworkCredential]::new('', $secureToken).Password
Remove-Variable secureToken
$secureFrontToken = Read-Host 'Token distinto per i client del proxy observer' -AsSecureString
$env:RUST_CPU_OBSERVER_TOKEN = [System.Net.NetworkCredential]::new('', $secureFrontToken).Password
Remove-Variable secureFrontToken
rust-cpu.exe --database "$dbPath" --observe-runtime lm-studio --observe-port 18434
```

La variabile contiene necessariamente il valore in memoria durante l'uso e può essere ereditata da processi figli. Al termine, se non serve ad altri processi avviati da quel terminale:

```powershell
Remove-Item Env:RUST_CPU_LM_STUDIO_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:RUST_CPU_OBSERVER_TOKEN -ErrorAction SilentlyContinue
```

Nel client compatibile OpenAI, lasciare la base URL `http://127.0.0.1:18434/v1` e impostare il campo API key al valore del token observer. Per un client HTTP, inviare l'header Bearer con quella stessa chiave; non usare il token upstream. I client che non consentono header di autenticazione non possono attraversare un proxy protetto. Le semplici prove `ollama list`/`ps` descritte sopra valgono per la modalità senza token observer.

Il listener resta raggiungibile dagli altri processi locali; quando il token observer è configurato, devono autenticarsi. Non esporlo tramite port forwarding o servizi di tunnel. Le richieste browser con header `Origin` vengono rifiutate; non è abilitato CORS.

## Rotte supportate

| Runtime | GET metadati | POST inferenza |
| --- | --- | --- |
| Ollama | `/api/tags`, `/api/ps`, `/api/version` | `/api/chat`, `/api/generate` |
| LM Studio | `/api/v1/models`, `/v1/models` | `/v1/chat/completions` |
| llama.cpp | `/health`, `/v1/health`, `/v1/models` | `/v1/chat/completions` |

Le altre rotte e le query string vengono rifiutate. I POST richiedono JSON valido, `Content-Type: application/json`, un campo `model` valido e un eventuale campo `stream` booleano. Non sono supportati download, upload di pesi, gestione amministrativa, tools/MCP del server o URL arbitrari forniti dal client.

## Significato delle metriche

- **Conteggio token:** soltanto i contatori restituiti dal runtime; nessuna conversione approssimativa dei caratteri in token.
- **TTFT:** primo contenuto/ragionamento non vuoto ricevuto dal proxy nello stream. Comprende lettura della richiesta e verifiche del proxy, coda, caricamento, prefill e trasporto. Per risposte JSON non streaming rimane n/d.
- **`native_decode`:** velocità dai contatori e dai tempi nativi Ollama/llama.cpp, quando disponibili.
- **`end_to_end`:** token di output dichiarati da `usage` divisi per la durata del proxy; include le fasi precedenti alla generazione. Non confrontarla direttamente con `native_decode`.
- **`unavailable`:** il protocollo non ha fornito dati sufficienti per una velocità. Il valore resta n/d.
- **Prefill:** su Ollama sottrae i token dichiarati in cache; senza contatore di cache la velocità prefill non viene calcolata.

Il corpo della risposta viene inoltrato progressivamente, senza sostituirlo con un testo generato dal programma. I parser osservano temporaneamente i byte già in transito; non persistono prompt, risposte, messaggi della conversazione, cookie o token. Una risposta HTTP fallita, uno stream troncato o malformato, un errore terminale o un trasferimento interrotto non produce una misura dichiarata riuscita. Il testo di errore restituito dal runtime può comunque essere inoltrato al client che ha effettuato la richiesta, senza copiarlo nell'archivio delle metriche.

## Limiti e arresto

Sono ammesse al massimo quattro richieste attive, inclusi gli stream; una quinta riceve HTTP 429. Il corpo di una richiesta è limitato a 1 MiB, i metadati a 2 MiB e la risposta inoltrata a 16 MiB. La raccolta delle metriche richiede righe/eventi stream fino a 64 KiB; una risposta non interpretabile può continuare a essere inoltrata entro il limite senza produrre metriche. Questi limiti possono escludere immagini o conversazioni molto grandi.

La connessione upstream ha timeout di un secondo, le letture hanno un limite di 30 secondi e ogni operazione ha un limite totale di 120 secondi. Premere **Ctrl+C nel terminale del proxy** per richiederne l'arresto. Le richieste in attesa vengono annullate e le connessioni residue vengono chiuse entro il limite di arresto del server.

Chiudere il proxy o disconnettere il client non termina il processo Ollama/LM Studio/llama.cpp. Il runtime può continuare un calcolo già avviato: il protocollo non fornisce una conferma universale di stop. Il programma garantisce i limiti e l'annullamento del proprio trasporto, non la configurazione interna del runtime né il suo eventuale instradamento verso servizi esterni.

I test HTTP locali del modulo verificano inoltro SSE/NDJSON, JSON non streaming, rifiuto delle rotte, limiti, concorrenza, interruzioni e assenza di contenuti privati nei risultati. Questi test con server simulati non certificano l'esecuzione di ogni versione di runtime o di ogni client esterno.
