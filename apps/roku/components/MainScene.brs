' Screens: server address → pairing code → home (Films, Séries) → episodes → video.
' The device token lives in the registry and is rotated a week before it expires.

sub init()
    m.global.addFields({ token: "" })
    m.registry = CreateObject("roRegistrySection", "aether")
    m.message = m.top.findNode("message")
    m.pairing = m.top.findNode("pairing")
    m.home = m.top.findNode("home")
    m.episodes = m.top.findNode("episodes")
    m.video = m.top.findNode("video")
    m.home.observeField("rowItemSelected", "onHomeSelected")
    m.episodes.observeField("itemSelected", "onEpisodeSelected")
    m.video.observeField("state", "onVideoState")
    m.video.observeField("position", "onVideoPosition")
    m.tasks = {}
    m.server = m.registry.Read("server")
    m.global.token = m.registry.Read("token")
    m.screen = ""
    if m.server = "" then
        askServer()
    else if m.global.token = "" then
        startPairing()
    else
        refreshIfDue()
    end if
end sub

' ---- HTTP -------------------------------------------------------------------

sub api(tag as String, method as String, path as String, body as Dynamic)
    task = CreateObject("roSGNode", "ApiTask")
    task.request = { server: m.server, token: m.global.token, method: method, path: path, body: body, tag: tag }
    task.observeField("response", "onResponse")
    m.tasks[tag] = task
    task.control = "run"
end sub

sub onResponse(event as Object)
    response = event.getData()
    m.tasks.Delete(response.tag)
    status = response.status
    json = response.json
    tag = response.tag
    if status = 401 and tag <> "poll" and tag <> "start" then
        forgetToken()
        startPairing()
        return
    end if
    if tag = "start" then
        onPairingStarted(status, json)
    else if tag = "poll" then
        onPollAnswer(status, json)
    else if tag = "refresh" then
        if status = 200 and json <> invalid then saveToken(json)
        loadHome()
    else if Left(tag, 8) = "catalog:" then
        onCatalog(Mid(tag, 9), status, json)
    else if tag = "detail" then
        onDetail(status, json)
    else if tag = "media" then
        onMedia(status, json)
    else if tag = "hls" then
        onHls(status, json)
    else if tag = "progress" then
        onProgress(status, json)
    else if tag = "save" then
        if status = 200 and json <> invalid and json.revision <> invalid then m.revision = json.revision
    end if
end sub

function encode(text as String) as String
    return CreateObject("roUrlTransfer").Escape(text)
end function

function nowSeconds() as Integer
    return CreateObject("roDateTime").AsSeconds()
end function

' ---- Server address and pairing --------------------------------------------

sub showMessage(text as String)
    m.message.text = text
    m.message.visible = true
end sub

sub askServer()
    m.screen = "server"
    dialog = CreateObject("roSGNode", "StandardKeyboardDialog")
    dialog.title = "Adresse de votre serveur Aether"
    dialog.message = ["Par exemple https://media.maison.fr"]
    dialog.text = "https://"
    dialog.buttons = ["Valider"]
    dialog.observeField("buttonSelected", "onServerEntered")
    m.top.dialog = dialog
end sub

sub onServerEntered()
    text = m.top.dialog.text.Trim()
    while Right(text, 1) = "/"
        text = Left(text, Len(text) - 1)
    end while
    m.top.dialog.close = true
    if Left(text, 8) <> "https://" and Left(text, 7) <> "http://" then
        askServer()
        return
    end if
    m.server = text
    m.registry.Write("server", text)
    m.registry.Flush()
    startPairing()
end sub

sub startPairing()
    m.screen = "pairing"
    m.home.visible = false
    m.episodes.visible = false
    showMessage("Connexion à " + m.server + "…")
    api("start", "POST", "/api/v1/auth/device/start", {})
end sub

sub onPairingStarted(status as Integer, json as Dynamic)
    if status <> 200 or json = invalid then
        showMessage("Serveur injoignable (" + status.ToStr() + "). Appuyez sur * pour changer d'adresse.")
        return
    end if
    m.secret = json.secret
    m.message.visible = false
    m.top.findNode("code").text = json.code
    m.top.findNode("link").text = "ou ouvrez " + json.url
    m.pairing.visible = true
    m.pollTimer = CreateObject("roSGNode", "Timer")
    m.pollTimer.duration = 3
    m.pollTimer.repeat = true
    m.pollTimer.observeField("fire", "poll")
    m.pollTimer.control = "start"
end sub

sub poll()
    if m.tasks.DoesExist("poll") then return
    api("poll", "POST", "/api/v1/auth/device/poll", { secret: m.secret, tokenType: "bearer" })
end sub

sub onPollAnswer(status as Integer, json as Dynamic)
    if status = 200 and json <> invalid and json.accessToken <> invalid then
        m.pollTimer.control = "stop"
        m.pairing.visible = false
        saveToken(json)
        loadHome()
    else if status = 410 or status = 404 then
        m.pollTimer.control = "stop"
        startPairing()
    end if
end sub

sub saveToken(json as Object)
    m.global.token = json.accessToken
    expires = nowSeconds() + 2592000
    if json.expiresIn <> invalid then expires = nowSeconds() + json.expiresIn
    m.registry.Write("token", json.accessToken)
    m.registry.Write("expires", expires.ToStr())
    m.registry.Flush()
end sub

sub forgetToken()
    m.global.token = ""
    m.registry.Delete("token")
    m.registry.Flush()
end sub

sub refreshIfDue()
    expires = m.registry.Read("expires").ToInt()
    if expires - nowSeconds() < 604800 then
        api("refresh", "POST", "/api/v1/auth/token/refresh", invalid)
    else
        loadHome()
    end if
end sub

' ---- Home -------------------------------------------------------------------

sub loadHome()
    m.screen = "home"
    showMessage("Chargement de vos bibliothèques…")
    m.rows = { film: invalid, series: invalid }
    api("catalog:film", "GET", "/api/v1/catalog?kind=film&offset=0&limit=200", invalid)
    api("catalog:series", "GET", "/api/v1/catalog?kind=series&offset=0&limit=200", invalid)
end sub

sub onCatalog(kind as String, status as Integer, json as Dynamic)
    row = CreateObject("roSGNode", "ContentNode")
    row.title = "Films"
    if kind = "series" then row.title = "Séries"
    if status = 200 and json <> invalid and json.items <> invalid then
        for each work in json.items
            item = row.createChild("ContentNode")
            item.id = work.id
            item.title = work.title
            item.addFields({ kind: work.kind })
            if work.posterUrl <> invalid then item.HDPosterUrl = m.server + work.posterUrl
        end for
    end if
    m.rows[kind] = row
    if m.rows.film = invalid or m.rows.series = invalid then return
    content = CreateObject("roSGNode", "ContentNode")
    for each key in ["film", "series"]
        if m.rows[key].getChildCount() > 0 then content.appendChild(m.rows[key])
    end for
    if content.getChildCount() = 0 then
        showMessage("Aucun film ni série accessible pour ce compte.")
        return
    end if
    m.message.visible = false
    m.home.content = content
    m.home.visible = true
    m.home.setFocus(true)
end sub

sub onHomeSelected()
    selection = m.home.rowItemSelected
    item = m.home.content.getChild(selection[0]).getChild(selection[1])
    if item.kind = "series" then
        m.screen = "episodes"
        showMessage(item.title)
        api("detail", "GET", "/api/v1/catalog/" + encode(item.id) + "?episodeLimit=100", invalid)
    else
        play(item.id, item.title)
    end if
end sub

sub onDetail(status as Integer, json as Dynamic)
    if status <> 200 or json = invalid or json.seasons = invalid then
        showMessage("Épisodes indisponibles.")
        return
    end if
    content = CreateObject("roSGNode", "ContentNode")
    for each season in json.seasons
        for each episode in season.episodes
            item = content.createChild("ContentNode")
            item.id = episode.id
            label = ""
            if season.number <> invalid then label = "S" + season.number.ToStr()
            if episode.number <> invalid then label = label + "E" + episode.number.ToStr()
            if label <> "" then label = label + " · "
            item.title = label + episode.title
        end for
    end for
    m.home.visible = false
    m.episodes.content = content
    m.episodes.visible = true
    m.episodes.setFocus(true)
end sub

sub onEpisodeSelected()
    item = m.episodes.content.getChild(m.episodes.itemSelected)
    play(item.id, item.title)
end sub

' ---- Playback ---------------------------------------------------------------

sub play(workId as String, title as String)
    m.playing = { workId: workId, title: title }
    m.revision = 0
    m.resumeAt = 0
    m.lastSaved = 0
    showMessage("Préparation de « " + title + " »…")
    api("progress", "GET", "/api/v1/progress/" + encode(workId), invalid)
    api("media", "GET", "/api/v1/media/" + encode(workId), invalid)
end sub

sub onProgress(status as Integer, json as Dynamic)
    if status = 200 and json <> invalid then
        if json.revision <> invalid then m.revision = json.revision
        if json.position <> invalid and json.watched <> true then m.resumeAt = json.position
    end if
end sub

sub onMedia(status as Integer, json as Dynamic)
    if status <> 200 or json = invalid or json.id = invalid then
        showMessage("Ce titre n'est pas disponible.")
        return
    end if
    api("hls", "POST", "/api/v1/assets/" + encode(json.id) + "/hls", invalid)
end sub

sub onHls(status as Integer, json as Dynamic)
    if status <> 200 or json = invalid or json.url = invalid then
        showMessage("Lecture impossible (" + status.ToStr() + ").")
        return
    end if
    content = CreateObject("roSGNode", "ContentNode")
    content.url = json.url
    content.streamFormat = "hls"
    content.title = m.playing.title
    m.message.visible = false
    m.video.content = content
    m.video.visible = true
    m.video.setFocus(true)
    if m.resumeAt > 5 then m.video.seek = m.resumeAt
    m.video.control = "play"
end sub

sub onVideoPosition()
    if m.video.position - m.lastSaved >= 15 then saveProgress(false)
end sub

sub onVideoState()
    state = m.video.state
    if state = "finished" then
        saveProgress(true)
        closeVideo()
    else if state = "error" then
        closeVideo()
        showMessage("Erreur de lecture : " + m.video.errorMsg)
    end if
end sub

sub saveProgress(watched as Boolean)
    if m.playing = invalid or m.video.duration <= 0 then return
    m.lastSaved = m.video.position
    body = { position: m.video.position, duration: m.video.duration, watched: watched, expectedRevision: m.revision }
    task = CreateObject("roSGNode", "ApiTask")
    task.request = { server: m.server, token: m.global.token, method: "PUT", path: "/api/v1/progress/" + encode(m.playing.workId), body: body, tag: "save" }
    task.observeField("response", "onResponse")
    m.tasks["save"] = task
    task.control = "run"
end sub

sub closeVideo()
    m.video.control = "stop"
    m.video.visible = false
    m.playing = invalid
    if m.episodes.visible then
        m.episodes.setFocus(true)
    else
        m.home.setFocus(true)
    end if
end sub

function onKeyEvent(key as String, press as Boolean) as Boolean
    if not press then return false
    if key = "back" then
        if m.video.visible then
            saveProgress(false)
            closeVideo()
            return true
        else if m.episodes.visible then
            m.episodes.visible = false
            m.message.visible = false
            m.home.visible = true
            m.home.setFocus(true)
            return true
        end if
    else if key = "options" and (m.screen = "pairing" or m.screen = "server") then
        if m.pollTimer <> invalid then m.pollTimer.control = "stop"
        m.pairing.visible = false
        askServer()
        return true
    end if
    return false
end function
