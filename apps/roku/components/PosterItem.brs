sub init()
    m.poster = m.top.findNode("poster")
    m.title = m.top.findNode("title")
end sub

sub showContent()
    content = m.top.itemContent
    if content = invalid then return
    m.title.text = content.title
    if m.global.token <> invalid and m.global.token <> "" then
        agent = CreateObject("roHttpAgent")
        agent.SetCertificatesFile("common:/certs/ca-bundle.crt")
        agent.InitClientCertificates()
        agent.AddHeader("Authorization", "Bearer " + m.global.token)
        m.poster.setHttpAgent(agent)
    end if
    m.poster.uri = content.HDPosterUrl
end sub
