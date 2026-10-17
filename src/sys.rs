use std::path::Path;

#[allow(dead_code)]
fn xq(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[allow(dead_code)]
fn sq(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$"))
}

pub fn autostart(on: bool, me: &Path, dd: &Path, port: u16) -> Result<String, String> {
    let exe = me.display().to_string();
    let mut av: Vec<String> = vec!["--data".into(), dd.display().to_string(), "--quiet".into()];
    if port != 47300 {
        av.extend(["--port".into(), port.to_string()]);
    }
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        let k = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", winreg::enums::KEY_SET_VALUE).map_err(|e| e.to_string())?;
        if on {
            k.set_value("ApolloOffice", &format!("\"{exe}\" {}", av.iter().map(|x| if x.starts_with("--") { x.clone() } else { format!("\"{x}\"") }).collect::<Vec<_>>().join(" "))).map_err(|e| e.to_string())?;
            return Ok("Starts at login from now on.".into());
        }
        let _ = k.delete_value("ApolloOffice");
        return Ok("Autostart off.".into());
    }
    #[cfg(target_os = "macos")]
    {
        let p = dirs::home_dir().ok_or("no home folder")?.join("Library/LaunchAgents/dev.apollonet.office.plist");
        if on {
            let x = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>dev.apollonet.office</string><key>ProgramArguments</key><array><string>{}</string>{}</array><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>StandardErrorPath</key><string>{}</string></dict></plist>\n",
                xq(&exe),
                av.iter().map(|x| format!("<string>{}</string>", xq(x))).collect::<String>(),
                xq(&dd.join("apollo-office.log").display().to_string())
            );
            std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
            std::fs::write(&p, x).map_err(|e| e.to_string())?;
            return Ok(format!("Starts at login from now on ({}).", p.display()));
        }
        let _ = std::fs::remove_file(&p);
        return Ok("Autostart off.".into());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let d = dirs::config_dir().ok_or("no config folder")?.join("systemd/user");
        let p = d.join("apollo-office.service");
        if on {
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            std::fs::write(&p, format!("[Unit]\nDescription=ApolloOffice\n\n[Service]\nExecStart={} {}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n", sq(&exe), av.iter().map(|x| sq(x)).collect::<Vec<_>>().join(" "))).map_err(|e| e.to_string())?;
            let _ = std::process::Command::new("systemctl").args(["--user", "daemon-reload"]).status();
            let _ = std::process::Command::new("systemctl").args(["--user", "enable", "apollo-office"]).status();
            return Ok("Starts at login from now on (systemd --user).".into());
        }
        let _ = std::process::Command::new("systemctl").args(["--user", "disable", "apollo-office"]).status();
        let _ = std::fs::remove_file(&p);
        Ok("Autostart off.".into())
    }
}

pub fn autostart_on() -> bool {
    #[cfg(windows)]
    {
        return winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run").and_then(|k| k.get_value::<String, _>("ApolloOffice")).is_ok();
    }
    #[cfg(target_os = "macos")]
    {
        return dirs::home_dir().map(|h| h.join("Library/LaunchAgents/dev.apollonet.office.plist").exists()).unwrap_or(false);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs::config_dir().map(|h| h.join("systemd/user/apollo-office.service").exists()).unwrap_or(false)
    }
}

