# Skylight

Un notch vivo in cima allo schermo di Windows: musica e testi sincronizzati,
Focus (Pomodoro), promemoria per bere, Shelf per i file, to-do con promemoria,
uso del PC e avvisi di sistema. Scritto in Rust, nativo, disegnato dalla GPU, e si attiva
solo quando succede qualcosa.

## Scaricare

1. Dalla pagina [Releases](https://github.com/LorenzoLubrano/skylight/releases) scarica
   `skylight-<versione>-win64.zip` ed estrailo dove vuoi tenerlo (per esempio in `Documenti\Skylight`).
2. Avvia `skylight.exe`. Funziona su Windows 10 e 11, senza installazione e senza permessi di amministratore.
3. L'eseguibile non è firmato, quindi al primo avvio Windows SmartScreen può mostrare un avviso:
   *Ulteriori informazioni* → *Esegui comunque*. Se preferisci, compila tu stesso il codice (vedi sotto).

Si esce dall'icona nella tray (*Esci*). Per toglierlo del tutto: spegni *Avvia con Windows* nelle impostazioni,
esci, poi cancella `skylight.exe`, la cartella `%APPDATA%\Skylight` e la chiave di registro `HKCU\Software\Skylight`.

## Cosa fa

### Notch chiuso
- **Ora e batteria** quando non succede niente: un anello verde mostra la carica (giallo sotto il 20%,
  rosso sotto il 10%). Se vuoi, anche la **data** ("lun 26 ott  17:21").
- **App a schermo intero** (video, giochi, presentazioni, browser con F11): il notch si riduce a una
  **linguetta** sottile; passandoci sopra col mouse si apre come sempre. In alternativa si può nasconderlo
  o lasciarlo com'è, e si può far ridurre anche con le **finestre ingrandite** (opzione nelle impostazioni).
- **Download in corso**: all'estrema destra compare un anello blu con la freccia, il cui arco gira finché
  il file arriva (senza percentuale: i browser non la comunicano ad altri programmi). Durante un download
  il notch resta normale anche a schermo intero. Vale per i file che arrivano nella cartella **Download**
  da Chrome, Edge, Brave, Vivaldi, Firefox e Opera (e qBittorrent/µTorrent se aggiungono l'estensione ai file incompleti).
- **Musica** in riproduzione: copertina, orologio (o la riga del testo sincronizzato) e tre barrette colorate (basse e grigie in pausa).
- **Focus attivo**: il conto alla rovescia prende il posto dell'orologio, con un anello che si svuota.
- **Avvisi** di volume, luminosità, Bloc Maiusc, lingua della tastiera, caricatore e batteria, Wi-Fi, cuffie Bluetooth
  e promemoria (Focus, acqua, to-do), questi ultimi con pulsanti di risposta. Un promemoria arrivato a schermo intero
  o con il notch aperto aspetta di essere visto invece di sparire.

### Notch aperto (passa il mouse)
In alto il titolo della pagina, l'anello della **batteria con la percentuale**, la **data**, l'ora e,
durante un download, il suo anello.
Ai lati due frecce per passare alla pagina precedente o successiva (anche con la rotella).
In basso i pallini delle pagine: portando il mouse lì diventano la **barra delle icone**, con l'ingranaggio
delle impostazioni.

| Pagina | Cosa contiene |
|---|---|
| **Musica** | Copertina, titolo, barra cliccabile, controlli e **volume** (clic sull'altoparlante: la barra regola il volume, anche con la rotella). Testi sincronizzati da LRCLIB (clic su una riga per saltare lì). Senza musica il lettore resta, con "Nessuna riproduzione" |
| **Focus** | Anello del tempo che resta (rosso concentrazione, verde pausa breve, blu pausa lunga), conto alla rovescia, sessioni, avvio-pausa, reset, salta e i pulsanti per passare alle altre fasi. Suono di Windows a fine fase |
| **Acqua** | Litri bevuti su obiettivo, interruttore dei promemoria con l'ora del prossimo, "Bevi 250 ml", annulla. A destra un indicatore circolare oppure (clic) il grafico ora per ora |
| **Shelf** | Trascina sul notch file, cartelle o testo; trascinali fuori in cartelle, email, chat. Clic per aprire (o copiare il testo), Ctrl+clic per selezionarne più di uno |
| **To-do** | Quante ne hai fatte oggi e la prossima scadenza; a destra la lista. Promemoria scritti in italiano: "Chiama Dina alle 15:30", "Riunione domani alle ore 9", "Consegna venerdì alle 12", "Cena stasera alle 9", "Palestra dopodomani mattina alle 7", "Tè tra 10 min", "Post +1h". All'ora giusta il notch avvisa, con "Fatto" e "Tra 10 minuti" (vale anche a cavallo del cambio dell'ora legale) |
| **Sistema** | CPU, RAM, disco, GPU, download e upload (con mini-grafici), consumo in watt, temperatura di CPU e GPU |

### Animazioni
Ispirate alla Dynamic Island e alle linee guida di Apple: molle regolate per durata e smorzamento,
apertura morbida con un piccolo rimbalzo, chiusura rapida e ferma, avvisi un po' più vivaci. Il contenuto
compare quando la forma è quasi aperta, crescendo leggermente dall'alto e mettendosi a fuoco (sfocatura
di Direct2D); cambiando pagina scorre di lato. Le animazioni si possono interrompere a metà, e se in Windows
è spento *Mostra animazioni* (Accessibilità → Effetti visivi) niente rimbalza, scorre o si sfoca.

### Impostazioni
Dall'ingranaggio nel notch o dalla tray (*Impostazioni…*). Ogni modifica si applica subito:
avvio con Windows, cosa fa il notch **a schermo intero** (si riduce, si nasconde, resta com'è) e se vale anche
per le finestre ingrandite, **data nel notch
chiuso**, **download in corso** nel notch, **monitor** su cui stare, **dimensione del notch** (70-140%), testi sincronizzati, quali avvisi mostrare,
durate del Focus, obiettivo, bicchiere, intervallo e fascia oraria dell'acqua.

## La pagina Sistema: da dove vengono i numeri

Tutto senza driver e senza permessi di amministratore, e solo mentre la pagina è aperta (una misura al secondo):

| Valore | Fonte |
|---|---|
| CPU, RAM | `GetSystemTimes`, `GlobalMemoryStatusEx` |
| Disco, GPU, memoria video | Contatori di prestazioni (gli stessi di Gestione attività) |
| Download / upload | `GetIfTable2`, solo schede di rete fisiche |
| Temperatura GPU | Driver video tramite D3DKMT, come Gestione attività (di solito le schede dedicate) |
| Temperatura CPU | Zona termica ACPI: **indicativa**, non è la temperatura del die |
| Consumo (W) | A batteria: la scarica della batteria (= tutto il portatile). Altrimenti, in ordine: il misuratore "Power Meter" di Windows (raro), i contatori di energia del processore (Energy Meter Interface, RAPL di Intel/AMD: tutta la piattaforma o il solo processore, anche col caricatore collegato), i sensori di LibreHardwareMonitor. Se il PC non ha nessuna di queste fonti, col caricatore collegato il riquadro suggerisce di staccarlo |

**Limite onesto:** Windows non offre a un'app normale né la temperatura esatta della CPU né il
consumo totale di un PC fisso. Programmi come HWiNFO o LibreHardwareMonitor ci arrivano con un driver
kernel (e quello più diffuso, WinRing0, è segnalato da Microsoft Defender come vulnerabile).
Per questo Skylight **non installa driver**. Però, **se LibreHardwareMonitor è aperto**, Skylight legge i suoi
sensori (via WMI) e mostra temperatura del package CPU e consumo di CPU + GPU esatti.

## Compilare ed eseguire (su Windows 10/11)

1. Installa Rust da <https://rustup.rs>. Durante l'installazione accetta i
   *Visual Studio Build Tools* con il carico "Sviluppo di applicazioni desktop con C++".
2. Dalla cartella del progetto:

```powershell
cargo run              # debug: con console, vedi eventuali errori dei thread
cargo build --release  # eseguibile finale in target\release\skylight.exe (circa 1,2 MB, con icona)
```

I test della logica (Pomodoro, acqua, to-do e il suo linguaggio per gli orari, shelf, avvisi,
impostazioni, layout, parser LRC, ricerca testi, formattazione, download, cambio di nome) girano ovunque:

```bash
cargo test
```

### Anteprime della grafica

`skylight.exe --snapshot <cartella>` disegna il notch in tutti gli stati, con dati di esempio, e salva
ogni immagine come `<nome>.bgra` (larghezza e altezza in u32, poi i pixel BGRA); le animazioni sono
sequenze di fotogrammi a 60 al secondo. Non modifica i tuoi dati (attività, acqua, Shelf, impostazioni)
e funziona anche con Skylight già aperto. Funziona anche con Wine,
dove però mancano la sfocatura e i font Segoe (si usano Selawik e le Fluent System Icons, se installati).

## Come è fatta

| Cartella / file | Ruolo |
|---|---|
| `src/features/` | Logica pura: `pomodoro`, `water`, `todo`, `shelf`, `alerts`, `sysinfo`, `downloads` |
| `src/layout/` | Geometria in DIP e hit-testing: pagine, avvisi, finestra Impostazioni |
| `src/settings.rs`, `src/store.rs` | Impostazioni (descritte da una tabella) e salvataggio JSON |
| `src/win/app/` | Finestra, ciclo dei messaggi, input, dati, avvisi, risvegli programmati |
| `src/win/render/` | Disegno Direct2D: notch, pagine, avvisi, finestra Impostazioni |
| `src/win/monitors/` | Volume (Core Audio), luminosità (WMI), rete (WinRT), Bluetooth (DeviceWatcher), download (eventi della cartella Download) |
| `src/win/sysmon.rs` | Misure della pagina Sistema |
| `src/win/dnd.rs`, `thumbs.rs` | Trascinamento OLE da/verso lo Shelf, anteprime della shell |
| `src/win/keyboard.rs`, `power.rs` | Bloc Maiusc e lingua, caricatore e batteria |
| `src/media/`, `src/lyrics/` | Musica (GSMTC) e testi (LRCLIB) |
| `tools/make_icon.py` | Rigenera le icone da `assets/skylight-1024.png` (`python tools/make_icon.py assets`, serve Pillow) |

### Perché è leggera
- **Niente webview**: DirectComposition + Direct2D, trasparenza vera.
- **Si disegna solo quando serve**: a riposo il thread dell'interfaccia dorme in `GetMessageW`.
- **Quasi niente polling**: volume, rete, Bluetooth, batteria e musica arrivano come eventi di Windows.
  I risvegli programmati sono solo quelli necessari (scatto del minuto, secondo del Focus se attivo,
  prossima riga del testo, prossimo promemoria), più un controllo leggerissimo dello schermo intero
  una volta al secondo: una finestra già in primo piano può passare a schermo intero (F11, il tasto "f"
  di YouTube) senza che Windows lo segnali. I download si scoprono dagli eventi della cartella
  Download; solo mentre c'è un file in arrivo se ne controlla la dimensione una volta al secondo.
- **La pagina Sistema misura solo mentre è aperta.**

## Privacy

- Nessun account, nessuna analisi d'uso.
- **Rete**: solo LRCLIB, e solo con i testi attivi (spenti di default). Invia titolo, artista, album e durata.
- **Tastiera**: per Bloc Maiusc e lingua Skylight riceve gli eventi della tastiera ("raw input"), ma guarda solo
  *quale* tasto è tra Bloc Maiusc, Shift, Ctrl, Alt, Win e Spazio. Nessun carattere viene letto, salvato o inviato.
  Spegnendo l'avviso nelle impostazioni l'ascolto viene disattivato del tutto.
- **Dati**: `%APPDATA%\Skylight\` (`todo.json`, `water.json`, `shelf.json`). Dello Shelf si salvano i percorsi dei file
  e, in chiaro, il testo che ci trascini (finché non lo togli). Impostazioni in `HKCU\Software\Skylight`.
  Se un file di dati risulta illeggibile non viene sovrascritto: resta da parte come `<nome>.bad`.

## Dal nome vecchio (Cresta)

Fino alla 0.3 l'app si chiamava Cresta. Al primo avvio Skylight copia dati, impostazioni e avvio con Windows
del nome vecchio, senza toccare gli originali; se qualcosa va storto parte con i valori predefiniti e riprova
all'avvio successivo. I dettagli sono in `src/migrate.rs` e `src/win/migration.rs`.

## Segnalare un problema

Apri una [issue](https://github.com/LorenzoLubrano/skylight/issues) con la versione di Windows, cosa hai
fatto e cosa è successo. Se l'hai compilato, lancia `cargo run` e allega quello che scrive la console.

## Limiti noti

- La lingua della tastiera si rileva quando la si cambia con le scorciatoie (Win+Spazio, Alt+Shift, Ctrl+Shift),
  non cliccando l'indicatore nella barra delle applicazioni.
- La batteria delle cuffie Bluetooth usa una proprietà non documentata di Windows: su alcuni modelli non compare.
- L'avviso della luminosità compare solo se hai usato mouse o tastiera negli ultimi secondi (così la luminosità
  automatica non lo fa scattare di continuo); si spegne a parte nelle impostazioni. Funziona con gli schermi
  dei portatili, non con i monitor esterni.
- Alt+F4 sul notch non chiude Skylight: si esce dalla tray (*Esci*).
- I download si vedono solo se il file arriva nella cartella Download (quella di Windows, anche se spostata).
  Steam, Microsoft Store e Windows Update usano cartelle proprie e non compaiono.
- "Oggi alle 9" scritto dopo le 9 avvisa subito; senza giorno, un orario già passato vale per domani.
- Il campo di testo del to-do è semplice (niente selezione col mouse); Ctrl+V incolla.

## Prossimi passi possibili

- Firma del codice (certificato o Azure Trusted Signing) per evitare l'avviso di SmartScreen.
- Un installer (es. WiX o Inno Setup) e gli aggiornamenti automatici.

## Crediti

Testi forniti da [LRCLIB](https://lrclib.net), servizio gratuito e comunitario.

## Licenza

Copyright © 2026 Lorenzo Lubrano.

Skylight è software libero: puoi ridistribuirlo e modificarlo secondo i termini della
[GNU General Public License](LICENSE), versione 3 o (a tua scelta) qualsiasi versione successiva.
Viene distribuito senza alcuna garanzia.
