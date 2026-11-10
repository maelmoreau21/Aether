sub init()
    m.top.functionName = "execute"
end sub

sub execute()
    request = m.top.request
    transfer = CreateObject("roUrlTransfer")
    port = CreateObject("roMessagePort")
    transfer.SetMessagePort(port)
    transfer.SetCertificatesFile("common:/certs/ca-bundle.crt")
    transfer.InitClientCertificates()
    transfer.EnableEncodings(true)
    transfer.RetainBodyOnError(true)
    transfer.SetUrl(request.server + request.path)
    transfer.AddHeader("Accept", "application/json")
    transfer.AddHeader("User-Agent", "Aether-Roku/1.0")
    if request.token <> invalid and request.token <> "" then
        transfer.AddHeader("Authorization", "Bearer " + request.token)
    end if
    method = request.method
    if method = invalid then method = "GET"
    transfer.SetRequest(method)
    started = false
    if method = "GET" then
        started = transfer.AsyncGetToString()
    else
        body = "{}"
        if request.body <> invalid then body = FormatJson(request.body)
        transfer.AddHeader("Content-Type", "application/json")
        started = transfer.AsyncPostFromString(body)
    end if
    status = 0
    text = ""
    if started then
        event = wait(20000, port)
        if type(event) = "roUrlEvent" then
            status = event.GetResponseCode()
            text = event.GetString()
        else
            transfer.AsyncCancel()
        end if
    end if
    json = invalid
    if text <> invalid and text <> "" then json = ParseJson(text)
    m.top.response = { tag: request.tag, status: status, json: json }
end sub
