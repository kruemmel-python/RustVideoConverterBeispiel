use eframe::{egui, App};
use rfd::FileDialog;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::path::Path;
use std::io::{BufRead, BufReader};

// Definieren der Nachrichtenstruktur
enum Message {
    Status(String),
    Progress(f32),
}

// Liste der unterstützten Ausgabeformate: (Anzeigename, Dateiendung)
const SUPPORTED_FORMATS: &[(&str, &str)] = &[
    ("MP4", ".mp4"),
    ("AVI", ".avi"),
    ("MKV", ".mkv"),
    ("MOV", ".mov"),
    ("WMV", ".wmv"),
    ("FLV", ".flv"),
];

struct VideoConverterApp {
    input_file: String,
    output_file: String,
    selected_format: usize, // Index der ausgewählten Formatoption
    status: String,
    progress: f32,          // Fortschritt in Prozent
    tx: Sender<Message>,    // Sender zum Senden von Nachrichten
    rx: Receiver<Message>,  // Receiver zum Empfangen von Nachrichten
    metadata_title: String,
    metadata_artist: String,
    metadata_description: String,
}

impl Default for VideoConverterApp {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            input_file: String::new(),
            output_file: String::new(),
            selected_format: 0, // Standardmäßig das erste Format (MP4) auswählen
            status: String::new(),
            progress: 0.0,
            tx,
            rx,
            metadata_title: String::new(),
            metadata_artist: String::new(),
            metadata_description: String::new(),
        }
    }
}

impl App for VideoConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Empfangene Nachrichten verarbeiten
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Status(status) => {
                    self.status = status;
                }
                Message::Progress(progress) => {
                    self.progress = progress;
                }
            }
            ctx.request_repaint(); // GUI aktualisieren
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Video Converter");

            // Eingabe-Datei auswählen
            ui.horizontal(|ui| {
                ui.label("Input File:");
                ui.text_edit_singleline(&mut self.input_file);
                if ui.button("Browse").clicked() {
                    if let Some(path) = FileDialog::new().pick_file() {
                        self.input_file = path.to_string_lossy().to_string();
                    }
                }
            });

            // Ausgabe-Datei auswählen
            ui.horizontal(|ui| {
                ui.label("Output File:");
                ui.text_edit_singleline(&mut self.output_file);
                if ui.button("Browse").clicked() {
                    if let Some(path) = FileDialog::new()
                        .add_filter("MP4", &["mp4"])
                        .add_filter("AVI", &["avi"])
                        .add_filter("MKV", &["mkv"])
                        .add_filter("MOV", &["mov"])
                        .add_filter("WMV", &["wmv"])
                        .add_filter("FLV", &["flv"])
                        .save_file()
                    {
                        self.output_file = path.to_string_lossy().to_string();
                    }
                }
            });

            // Ausgabeformat auswählen
            ui.horizontal(|ui| {
                ui.label("Output Format:");
                egui::ComboBox::from_id_salt("output_format_salt")
                    .selected_text(SUPPORTED_FORMATS[self.selected_format].0)
                    .show_ui(ui, |ui| {
                        for (index, (format_name, _)) in SUPPORTED_FORMATS.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_format, index, *format_name);
                        }
                    });
            });

            // Metadata input
            ui.separator();
            ui.heading("Metadata");

            ui.horizontal(|ui| {
                ui.label("Title:");
                ui.text_edit_singleline(&mut self.metadata_title);
            });

            ui.horizontal(|ui| {
                ui.label("Artist:");
                ui.text_edit_singleline(&mut self.metadata_artist);
            });

            ui.horizontal(|ui| {
                ui.label("Description:");
                ui.text_edit_multiline(&mut self.metadata_description);
            });

            // Fortschrittsanzeige
            ui.horizontal(|ui| {
                ui.label("Progress:");
                ui.add(egui::ProgressBar::new(self.progress / 100.0).show_percentage());
            });

            // Konvertierung starten
            if ui.button("Convert").clicked() {
                let input = self.input_file.clone();
                let mut output = self.output_file.clone();
                let tx = self.tx.clone();
                let selected_extension = SUPPORTED_FORMATS[self.selected_format].1.to_string();
                let metadata_title = self.metadata_title.clone();
                let metadata_artist = self.metadata_artist.clone();
                let metadata_description = self.metadata_description.clone();

                // Überprüfe, ob die Ausgabedatei eine Erweiterung hat
                if Path::new(&output).extension().is_none() {
                    // Füge die ausgewählte Erweiterung hinzu
                    output.push_str(&selected_extension);
                } else {
                    // Überprüfe, ob die Erweiterung mit dem ausgewählten Format übereinstimmt
                    let path = Path::new(&output);
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        let selected_ext_str = selected_extension.trim_start_matches('.').to_lowercase();
                        if ext_str != selected_ext_str {
                            // Ersetze die falsche Erweiterung durch die richtige
                            if let Some(stem) = path.file_stem() {
                                output = stem.to_string_lossy().to_string();
                                output.push_str(&selected_extension);
                            }
                        }
                    }
                }

                // Aktualisiere das output_file Feld, falls die Erweiterung hinzugefügt oder ersetzt wurde
                self.output_file = output.clone();

                // Setze Fortschritt und Status zurück
                self.progress = 0.0;
                self.status = "Starting conversion...".to_string();

                // Hintergrund-Thread starten für die Videokonvertierung
                thread::spawn(move || {
                    if input.is_empty() || output.is_empty() {
                        let _ = tx.send(Message::Status("Please specify both input and output files.".to_string()));
                        return;
                    }

                    // Ermitteln der Gesamtdauer des Videos
                    let duration_output = Command::new("ffmpeg")
                        .arg("-i")
                        .arg(&input)
                        .stderr(Stdio::piped())
                        .output();

                    let total_duration = match duration_output {
                        Ok(output) => {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            // Suche nach der Zeile, die die Dauer enthält
                            if let Some(duration_line) = stderr.lines().find(|line| line.contains("Duration:")) {
                                // Beispielzeile: " Duration: 00:04:36.10, start: 0.000000, bitrate: 2018 kb/s"
                                if let Some(start) = duration_line.find("Duration: ") {
                                    let duration_str = &duration_line[start + 10..start + 19]; // "00:04:36.10"
                                    if let Ok(total_seconds) = parse_duration(duration_str) {
                                        total_seconds
                                    } else {
                                        let _ = tx.send(Message::Status("Failed to parse video duration.".to_string()));
                                        return;
                                    }
                                } else {
                                    let _ = tx.send(Message::Status("Failed to find duration information.".to_string()));
                                    return;
                                }
                            } else {
                                let _ = tx.send(Message::Status("Failed to retrieve video duration.".to_string()));
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Message::Status(format!("Failed to execute ffmpeg for duration: {}", e)));
                            return;
                        }
                    };

                    // Starte die eigentliche Konvertierung und überwache den Fortschritt
                    let mut ffmpeg_command = Command::new("ffmpeg");
                    ffmpeg_command
                        .arg("-i")
                        .arg(&input)
                        .arg(&output);

                    // Füge die Metadaten hinzu, falls sie vorhanden sind
                    if !metadata_title.is_empty() {
                        ffmpeg_command = ffmpeg_command.arg("-metadata").arg(format!("title={}", metadata_title));
                    }
                    if !metadata_artist.is_empty() {
                        ffmpeg_command = ffmpeg_command.arg("-metadata").arg(format!("artist={}", metadata_artist));
                    }
                    if !metadata_description.is_empty() {
                        ffmpeg_command = ffmpeg_command.arg("-metadata").arg(format!("description={}", metadata_description));
                    }

                    ffmpeg_command
                        .stderr(Stdio::piped())
                        .stdout(Stdio::null());

                    let mut ffmpeg_process = match ffmpeg_command.spawn() {
                        Ok(process) => process,
                        Err(e) => {
                            let _ = tx.send(Message::Status(format!("Failed to start ffmpeg: {}", e)));
                            return;
                        }
                    };

                    let stderr = ffmpeg_process.stderr.take().expect("Failed to capture stderr");
                    let reader = BufReader::new(stderr);

                    for line in reader.lines() {
                        if let Ok(line) = line {
                            // Suche nach "time=HH:MM:SS.xx"
                            if let Some(time_pos) = line.find("time=") {
                                let time_str = &line[time_pos + 5..];
                                // Extrahiere die Zeit bis zum nächsten Komma oder Leerzeichen
                                let end_pos = time_str.find(|c: char| c == ' ' || c == ',').unwrap_or(time_str.len());
                                let time = &time_str[..end_pos];
                                if let Ok(current_seconds) = parse_duration(time) {
                                    let progress = (current_seconds / total_duration) * 100.0;
                                    let progress = if progress > 100.0 { 100.0 } else { progress };
                                    let _ = tx.send(Message::Progress(progress));
                                }
                            }
                        }
                    }

                    // Warte auf das Ende des ffmpeg-Prozesses
                    match ffmpeg_process.wait() {
                        Ok(status) => {
                            if status.success() {
                                let _ = tx.send(Message::Status("Conversion successful.".to_string()));
                                let _ = tx.send(Message::Progress(100.0));
                            } else {
                                let _ = tx.send(Message::Status("Conversion failed.".to_string()));
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Message::Status(format!("Failed to wait on ffmpeg: {}", e)));
                        }
                    }
                });
            }

            // Status und Fortschritt anzeigen
            ui.separator();
            ui.label("Status:");
            ui.label(&self.status);
        });
    }
}

// Funktion zur Umwandlung von "HH:MM:SS.xx" in Sekunden
fn parse_duration(duration: &str) -> Result<f32, ()> {
    let parts: Vec<&str> = duration.split(':').collect();
    if parts.len() != 3 {
        return Err(());
    }

    let hours: f32 = parts[0].parse().map_err(|_| ())?;
    let minutes: f32 = parts[1].parse().map_err(|_| ())?;
    let seconds: f32 = parts[2].parse().map_err(|_| ())?;

    Ok(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Video Converter",
        native_options,
        Box::new(|_cc| Ok(Box::new(VideoConverterApp::default()))),
    )
    .expect("Failed to run eframe");
}
