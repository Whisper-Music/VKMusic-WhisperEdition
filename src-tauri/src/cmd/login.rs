//! Вход в сервисы (ВКонтакте, SoundCloud): страница входа вторым webview, куки WebKit / WebView2.
//! Команды Tauri (#[tauri::command]) — список в generate_handler! в main.rs; общее (AppState, Res, …) — из main.rs.

use crate::*;

// ───────────────────────── вход в сервисы (ВКонтакте, SoundCloud) ─────────────────────────

/// Высота полосы интерфейса над страницей входа (подсказка и «Отмена»), как в NK-Player.
pub(crate) const LOGIN_BAR: i32 = 56;
/// webview страницы входа (одна на все сервисы)
pub(crate) const LOGIN_LABEL: &str = "svc-login";

/// Страница входа внутри главного окна (как WebContentsView в NK-Player). В GTK оба webview лежат в
/// вертикальном боксе окна; на время входа переносим их в `gtk::Paned`: сверху полоса интерфейса
/// высотой `LOGIN_BAR` (подсказка и «Отмена»), снизу страница сервиса. После входа интерфейс возвращается в бокс.
/// (Упаковкой бокса не обойтись: GTK отдаёт WebKitWebView его естественную высоту — всё окно.)
pub(crate) fn login_layout(app: &AppHandle, login: bool) {
    // интерфейс в браузере (--host): страница входа — отдельным окном, раскладывать нечего
    if host::active() {
        return;
    }
    // Windows: страница входа — дочерний webview поверх интерфейса (ниже полосы), раскладывать нечего
    #[cfg(not(target_os = "linux"))]
    let _ = (app, login);
    #[cfg(target_os = "linux")]
    let Some(w) = app.get_window("main") else { return };
    #[cfg(target_os = "linux")]
    let _ = app.run_on_main_thread(move || {
        use gtk::prelude::*;
        let Ok(vbox) = w.default_vbox() else { return };
        let paned = vbox.children().into_iter().find_map(|c| c.downcast::<gtk::Paned>().ok());
        if login {
            if paned.is_some() {
                return;
            }
            let kids = vbox.children();
            let (Some(ui), Some(page)) = (kids.first(), kids.get(1)) else { return };
            let p = gtk::Paned::new(gtk::Orientation::Vertical);
            vbox.remove(ui);
            vbox.remove(page);
            p.pack1(ui, false, true);
            p.pack2(page, true, false);
            p.set_position(LOGIN_BAR);
            vbox.pack_start(&p, true, true, 0);
            p.show_all();
        } else if let Some(p) = paned {
            // все webview — обратно в бокс; страницу входа Tauri закроет следом
            let kids = p.children();
            for k in &kids {
                p.remove(k);
            }
            vbox.remove(&p);
            for (i, k) in kids.iter().enumerate() {
                vbox.pack_start(k, i == 0, true, 0);
            }
        }
    });
}

pub(crate) fn login_close(app: &AppHandle) {
    if host::active() {
        if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
            let _ = w.close();
        }
        return;
    }
    if let Some(v) = app.get_webview(LOGIN_LABEL) {
        let _ = v.close();
    }
    login_layout(app, false);
}

/// Хранилище кук WebKit (общее для всех webview приложения): `f` выполняется в главном потоке и
/// отвечает через канал. Сам вызов — не из главного потока (иначе ждали бы сами себя).
/// get_webview, а не get_webview_window: пока открыт вход, в окне main два webview.
/// Не `Webview::cookies` из wry: он отдаёт домен без ведущей точки («.vk.ru» → «vk.ru»), и кука сайта
/// становится кукой одного хоста — не уходит на login.vk.ru, где ВК после перезапуска обновляет сессию
/// (было: «Сессия ВКонтакте не распознана» до «Выйти»); и удалить такую куку через wry нельзя.
#[cfg(target_os = "linux")]
pub(crate) fn with_cookie_manager<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(webkit2gtk::CookieManager, std::sync::mpsc::Sender<Res<T>>) + Send + 'static,
) -> Res<T> {
    let w = app.get_webview("main").ok_or("Нет главного окна")?;
    let (tx, rx) = std::sync::mpsc::channel();
    let fail = tx.clone();
    w.with_webview(move |pw| {
        use webkit2gtk::{WebViewExt, WebsiteDataManagerExt};
        match pw.inner().website_data_manager().and_then(|m| m.cookie_manager()) {
            Some(cm) => f(cm, tx),
            None => {
                let _ = fail.send(Err("Куки недоступны".into()));
            }
        }
    })
    .map_err(|e| format!("Куки недоступны: {e}"))?;
    rx.recv_timeout(std::time::Duration::from_secs(10)).map_err(|_| "Куки недоступны: нет ответа WebKit".to_string())?
}

/// webkit_cookie_manager_get_all_cookies: в webkit2gtk 2.0 обёртки нет, у wry она закрытая — та же привязка.
#[cfg(target_os = "linux")]
pub(crate) mod wk {
    use gtk::gio;
    use gtk::glib::{self, translate::*};
    use webkit2gtk::ffi::WebKitCookieManager;

    unsafe extern "C" {
        fn webkit_cookie_manager_get_all_cookies(
            cm: *mut WebKitCookieManager,
            cancellable: *mut gio::ffi::GCancellable,
            callback: gio::ffi::GAsyncReadyCallback,
            data: glib::ffi::gpointer,
        );
        fn webkit_cookie_manager_get_all_cookies_finish(
            cm: *mut WebKitCookieManager,
            res: *mut gio::ffi::GAsyncResult,
            err: *mut *mut glib::ffi::GError,
        ) -> *mut glib::ffi::GList;
    }

    type Done = Box<dyn FnOnce(Result<Vec<soup::Cookie>, glib::Error>)>;

    unsafe extern "C" fn finish(src: *mut glib::gobject_ffi::GObject, res: *mut gio::ffi::GAsyncResult, data: glib::ffi::gpointer) {
        unsafe {
            let mut err = std::ptr::null_mut();
            let list = webkit_cookie_manager_get_all_cookies_finish(src as *mut _, res, &mut err);
            let r = if err.is_null() { Ok(FromGlibPtrContainer::from_glib_full(list)) } else { Err(from_glib_full(err)) };
            let done: Box<Done> = Box::from_raw(data as *mut Done);
            done(r);
        }
    }

    /// Все куки хранилища. Только из главного потока (колбэк — там же).
    pub fn all_cookies(cm: &webkit2gtk::CookieManager, done: impl FnOnce(Result<Vec<soup::Cookie>, glib::Error>) + 'static) {
        let data: Box<Done> = Box::new(Box::new(done));
        unsafe {
            webkit_cookie_manager_get_all_cookies(
                cm.to_glib_none().0,
                std::ptr::null_mut(),
                Some(finish),
                Box::into_raw(data) as glib::ffi::gpointer,
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn plain_cookie(c: &mut soup::Cookie) -> vk::Cookie {
    vk::Cookie {
        name: c.name().map(|s| s.to_string()).unwrap_or_default(),
        value: c.value().map(|s| s.to_string()).unwrap_or_default(),
        domain: c.domain().map(|s| s.to_string()).unwrap_or_default(),
        path: c.path().map(|s| s.to_string()).unwrap_or_else(|| "/".into()),
    }
}

/// Все куки WebKit с настоящими доменами (с точкой — кука сайта и его поддоменов).
#[cfg(target_os = "linux")]
pub(crate) fn all_cookies(app: &AppHandle) -> Res<Vec<vk::Cookie>> {
    with_cookie_manager(app, |cm, tx| {
        wk::all_cookies(&cm, move |r| {
            let r = r.map(|mut v| v.iter_mut().map(plain_cookie).collect()).map_err(|e| format!("Куки недоступны: {e}"));
            let _ = tx.send(r);
        });
    })
}

/// Удалить куки, домен которых подходит под `own` (выход из сервиса).
#[cfg(target_os = "linux")]
pub(crate) fn delete_cookies(app: &AppHandle, own: fn(&str) -> bool) -> Res<()> {
    with_cookie_manager(app, move |cm, tx| {
        use webkit2gtk::CookieManagerExt;
        let mgr = cm.clone();
        wk::all_cookies(&cm, move |r| {
            let mut all = match r {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(format!("Куки недоступны: {e}")));
                    return;
                }
            };
            all.retain_mut(|c| c.domain().is_some_and(|d| own(d.as_str())));
            let left = std::rc::Rc::new(std::cell::Cell::new(all.len()));
            if all.is_empty() {
                let _ = tx.send(Ok(()));
                return;
            }
            for mut c in all {
                let (left, tx) = (left.clone(), tx.clone());
                mgr.delete_cookie(&mut c, None::<&gtk::gio::Cancellable>, move |r| {
                    if let Err(e) = r {
                        let _ = tx.send(Err(format!("Не удалось удалить куки: {e}")));
                    }
                    left.set(left.get() - 1);
                    if left.get() == 0 {
                        let _ = tx.send(Ok(()));
                    }
                });
            }
        });
    })
}

/// Куки WebView2 (Windows): как на Linux, напрямую из хранилища — `ICoreWebView2CookieManager`, с доменом как есть
/// (у куки сайта — с точкой); удаляется та же самая кука, что пришла из хранилища.
#[cfg(windows)]
pub(crate) mod wv2 {
    use super::Res;
    use tauri::{AppHandle, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Cookie, ICoreWebView2CookieManager, ICoreWebView2_2};
    use webview2_com::{take_pwstr, GetCookiesCompletedHandler};
    use windows::core::{Interface, PCWSTR, PWSTR};

    /// `f` получает все куки хранилища — в главном потоке; сам вызов — не из главного потока.
    fn with_cookies<T: Send + 'static>(
        app: &AppHandle,
        f: impl FnOnce(ICoreWebView2CookieManager, Vec<ICoreWebView2Cookie>) -> Res<T> + Send + 'static,
    ) -> Res<T> {
        let w = app.get_webview("main").ok_or("Нет главного окна")?;
        let (tx, rx) = std::sync::mpsc::channel::<Res<T>>();
        w.with_webview(move |pw| {
            let cm = unsafe { pw.controller().CoreWebView2().and_then(|v| v.cast::<ICoreWebView2_2>()).and_then(|v| v.CookieManager()) };
            let cm = match cm {
                Ok(cm) => cm,
                Err(e) => {
                    let _ = tx.send(Err(format!("Куки недоступны: {e}")));
                    return;
                }
            };
            let mgr = cm.clone();
            let fail = tx.clone();
            let done = GetCookiesCompletedHandler::create(Box::new(move |code, list| {
                let r = (|| -> windows::core::Result<Vec<ICoreWebView2Cookie>> {
                    code?;
                    let mut out = Vec::new();
                    if let Some(list) = list {
                        let mut n = 0u32;
                        unsafe { list.Count(&mut n)? };
                        for i in 0..n {
                            out.push(unsafe { list.GetValueAtIndex(i)? });
                        }
                    }
                    Ok(out)
                })();
                let _ = tx.send(r.map_err(|e| format!("Куки недоступны: {e}")).and_then(|v| f(mgr, v)));
                Ok(())
            }));
            if let Err(e) = unsafe { cm.GetCookies(PCWSTR::null(), &done) } {
                let _ = fail.send(Err(format!("Куки недоступны: {e}")));
            }
        })
        .map_err(|e| format!("Куки недоступны: {e}"))?;
        rx.recv_timeout(std::time::Duration::from_secs(10)).map_err(|_| "Куки недоступны: нет ответа WebView2".to_string())?
    }

    fn text(get: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> String {
        let mut p = PWSTR::null();
        if get(&mut p).is_err() {
            return String::new();
        }
        take_pwstr(p)
    }

    fn plain(c: &ICoreWebView2Cookie) -> crate::vk::Cookie {
        unsafe {
            crate::vk::Cookie {
                name: text(|p| c.Name(p)),
                value: text(|p| c.Value(p)),
                domain: text(|p| c.Domain(p)),
                path: Some(text(|p| c.Path(p))).filter(|p| !p.is_empty()).unwrap_or_else(|| "/".into()),
            }
        }
    }

    pub fn all_cookies(app: &AppHandle) -> Res<Vec<crate::vk::Cookie>> {
        with_cookies(app, |_, all| Ok(all.iter().map(plain).collect()))
    }

    pub fn delete_cookies(app: &AppHandle, own: fn(&str) -> bool) -> Res<()> {
        with_cookies(app, move |cm, all| {
            for c in all.iter().filter(|c| own(&plain(c).domain)) {
                unsafe { cm.DeleteCookie(c) }.map_err(|e| format!("Не удалось удалить куки: {e}"))?;
            }
            Ok(())
        })
    }
}
// куки нужны и другим разделам (vk_cmd, sc_cmd) — через `use crate::*`
#[cfg(windows)]
pub(crate) use wv2::{all_cookies, delete_cookies};

/// Вход выполнен, если у сервиса есть кука сессии.
pub(crate) fn logged_in(app: &AppHandle, service: &str) -> bool {
    match service {
        "vk" => matches!(vk_cookie(app), Ok(Some(_))),
        "sc" => matches!(sc_token(app), Ok(Some(_))),
        _ => false,
    }
}

/// Обычная страница входа сервиса (ВК — можно по QR-коду) прямо в главном окне.
/// Прав на команды у неё нет (capabilities — только main).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn svc_login(app: AppHandle, service: String) -> Res<()> {
    let (url, title) = match service.as_str() {
        "vk" => (vk::LOGIN_URL, "ВКонтакте"),
        "sc" => (sc::LOGIN_URL, "SoundCloud"),
        _ => return Err("Неизвестный сервис".into()),
    };
    if app.get_webview(LOGIN_LABEL).is_some() {
        return Ok(());
    }
    let url = tauri::Url::parse(url).map_err(|e| e.to_string())?;
    let popups = app.clone();
    let seq = AtomicU64::new(0);
    // всплывающие окна (VK ID, вход через Google/Apple) открываются отдельными окнами
    let popup = move |url: tauri::Url, features: tauri::webview::NewWindowFeatures| {
        let label = format!("svc-popup-{}", seq.fetch_add(1, Ordering::Relaxed));
        match tauri::WebviewWindowBuilder::new(&popups, label, tauri::WebviewUrl::External(url))
            .window_features(features)
            .title(title)
            .user_agent(vk::USER_AGENT)
            .build()
        {
            Ok(window) => tauri::webview::NewWindowResponse::Create { window },
            Err(_) => tauri::webview::NewWindowResponse::Deny,
        }
    };
    if host::active() {
        // интерфейс в браузере (--host): страница входа — своим окном; закрыли окно — вход отменён
        tauri::WebviewWindowBuilder::new(&app, LOGIN_LABEL, tauri::WebviewUrl::External(url))
            .title(title)
            .inner_size(900.0, 760.0)
            .user_agent(vk::USER_AGENT)
            .on_new_window(popup)
            .build()
            .map_err(|e| format!("Не удалось открыть страницу входа: {e}"))?;
    } else {
        let main = app.get_window("main").ok_or("Нет главного окна")?;
        let builder = tauri::webview::WebviewBuilder::new(LOGIN_LABEL, tauri::WebviewUrl::External(url))
            // как в NK-Player (Electron): со своим UA WebKitGTK страница vk.ru/login не догружается
            .user_agent(vk::USER_AGENT)
            .on_new_window(popup);
        // Windows: GTK-раскладки нет — страница входа сама следует за размером окна
        #[cfg(windows)]
        let builder = builder.auto_resize();
        let size = main.inner_size().map_err(|e| e.to_string())?;
        let scale = main.scale_factor().unwrap_or(1.0);
        let size = size.to_logical::<f64>(scale);
        main.add_child(
            builder,
            tauri::LogicalPosition::new(0.0, LOGIN_BAR as f64),
            tauri::LogicalSize::new(size.width, (size.height - LOGIN_BAR as f64).max(100.0)),
        )
        .map_err(|e| format!("Не удалось открыть страницу входа: {e}"))?;
        login_layout(&app, true);
    }
    let st = app.state::<AppState>();
    st.vk.forget();
    st.sc.forget();

    // как в NK-Player: опрашиваем куки, пока не появится сессия, затем убираем страницу входа
    let h = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        if h.get_webview(LOGIN_LABEL).is_none() {
            let _ = h.emit("svc://event", serde_json::json!({ "type": "login_closed", "service": service }));
            break;
        }
        if logged_in(&h, &service) {
            if service == "vk" {
                if let Ok(Some(session)) = vk_cookie(&h) {
                    // после входа ВК доводит сессию редиректами login.php?act=slogin, выставляя куки;
                    // даём странице входа пройти их, как браузеру, — тогда полные куки сохранятся в WebKit
                    vk_login_settle(&h, session.base);
                }
            }
            login_close(&h);
            let _ = h.emit("svc://event", serde_json::json!({ "type": "login", "service": service }));
            break;
        }
    });
    Ok(())
}

/// Открыть /audio на странице входа и дождаться, пока она туда дойдёт (до 15 с).
pub(crate) fn vk_login_settle(app: &AppHandle, base: &str) {
    let Some(v) = app.get_webview(LOGIN_LABEL) else { return };
    let Ok(target) = tauri::Url::parse(&format!("{base}/audio")) else { return };
    if v.navigate(target.clone()).is_err() {
        return;
    }
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(15) {
        std::thread::sleep(std::time::Duration::from_millis(500));
        match v.url() {
            Ok(u) if u.host_str() == target.host_str() && u.path() == "/audio" => {
                // страница загружена — даём ей выставить куки
                std::thread::sleep(std::time::Duration::from_millis(1500));
                return;
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) fn svc_login_cancel(app: AppHandle) {
    login_close(&app);
}

/// Выход: удалить куки сервиса из хранилища WebKit.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn svc_logout(app: AppHandle, st: State<'_, AppState>, service: String) -> Res<()> {
    let own: fn(&str) -> bool = match service.as_str() {
        "vk" => vk::is_vk_domain,
        "sc" => sc::is_sc_domain,
        _ => return Err("Неизвестный сервис".into()),
    };
    st.vk.forget();
    st.sc.forget();
    // синхронизированные треки и ссылки на потоки уходят вместе с сессией
    st.saved.clear_synced(&service)?;
    prune_order(&st, &service)?;
    st.streams.clear_prefix(&format!("{service}:"));
    if service == "vk" {
        st.streams.clear_prefix("vkapi:");
        st.playlists.clear();
    }
    let h = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Res<()> {
        login_close(&h);
        delete_cookies(&h, own)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_search(app: AppHandle, st: State<'_, AppState>, query: String, more: Option<vk::More>, live: Option<bool>) -> Res<vk::Page> {
    let query = query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err("Введите запрос до 200 символов".into());
    }
    if more.as_ref().is_some_and(|m| m.section_id.len() > 200 || m.start_from.len() > 200) {
        return Err("Неверное продолжение поиска".into());
    }
    let session = vk_session(&app).await?;
    // live — из строки поиска (пока печатают): устаревший запрос в очереди не ждёт, см. Vk::search
    st.vk.search(&session, query, more, live == Some(true)).await
}

/// Играть трек ВК: ссылку получает Rust (одну, по нажатию), интерфейс её не видит.
/// Ссылка уже прослушанного трека берётся из кэша; fresh — не брать из кэша (повтор, если не открылась).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn vk_play(app: AppHandle, st: State<'_, AppState>, reload: String, title: Option<String>, fresh: Option<bool>) -> Res<bool> {
    let p = st.player()?;
    st.cache.begin();
    let key = format!("vk:{reload}");
    let title = title.unwrap_or_default();
    // трек библиотеки играет из кэша; не открылся (fresh) — кэш выбрасываем
    let saved = st.saved.list().into_iter().find(|s| s.service == "vk" && s.reload.as_deref() == Some(reload.as_str()));
    let ckey = saved.as_ref().map(|s| cache::service_key("vk", &s.id));
    if let Some(k) = &ckey {
        if fresh == Some(true) {
            st.cache.remove(k);
        } else if let Some(c) = st.cache.get(k) {
            {
                st.cache.touch(&c);
                return p.load(&c.to_string_lossy(), &title).map(|_| false);
            }
        }
    }
    let s = match st.streams.get(&key) {
        Some(s) if fresh != Some(true) => s,
        _ => {
            st.streams.forget(&key);
            vk_stream(&app, &st, &reload).await?
        }
    };
    p.load_stream(&s.url, &title, vk::USER_AGENT, &s.referrer)?;
    if let (Some(k), Some(t)) = (ckey, saved) {
        st.cache.enqueue(cache_job(k, &s, &t));
        return Ok(true);
    }
    Ok(false)
}

/// Новая ссылка на поток ВК (запрос к сервису) — сразу в кэш ссылок.
pub(crate) async fn vk_stream(app: &AppHandle, st: &AppState, reload: &str) -> Res<streams::Stream> {
    let session = vk_session(app).await?;
    let url = st.vk.stream_url(&session, reload).await?;
    let s = streams::Stream { url, referrer: session.referrer() };
    st.streams.put(&format!("vk:{reload}"), s.clone());
    Ok(s)
}

pub(crate) fn cache_job(key: String, s: &streams::Stream, t: &saved::Saved) -> cache::Job {
    cache::Job {
        key,
        lib: saved_key(t),
        source: cache::Source::Stream { url: s.url.clone(), referrer: s.referrer.clone() },
        artist: t.artist.clone(),
        title: t.title.clone(),
    }
}
