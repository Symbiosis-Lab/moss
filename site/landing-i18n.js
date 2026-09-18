(() => {
  const supported = new Set(['en', 'zh-hans', 'zh-hant']);
  const requested = new URLSearchParams(location.search).get('lang');
  const routeLocale = location.pathname.match(/^\/(zh-hans|zh-hant)(?:\/|$)/)?.[1] || (location.pathname === '/' ? 'en' : null);
  let stored = null; try { stored = localStorage.getItem('moss-landing-locale'); } catch {}
  const initial = requested === 'zh' ? 'zh-hans' : supported.has(requested) ? requested : routeLocale || (supported.has(stored) ? stored : 'en');
  window.__LANDING_LOCALE = initial;
  window.__LANG = initial === 'en' ? 'en' : 'zh';
  document.documentElement.lang = initial;
  const en = {
    product:'moss',
    title:'moss — Publish your site from your folder',description:'Write in your folder and publish to your internet. moss turns Markdown, media, and creative code into a website you own.',
    intro:'Publish your site<br>from your folder.',h1:'Write anywhere.<br>Publish everywhere.',b1:'Write in your folder with Markdown files. Collaborate through any cloud drive. One click to preview, another to publish anywhere.',h2:'Friendly for humans.<br>Powerful for AI.',b2:'Everything lives in local files, ready for your favorite editor or coding agent. Instant previews and an intuitive interface make your site pleasant to use and effortless to maintain.',h3:'As light as your folder.<br>As vast as the internet.',b3:'moss optimizes and packages your files into a website, from prose and video to JavaScript sketches and Jupyter notebooks.',h4:'Publish to your internet,<br>your way.',b4:'Choose your host and sync the same content to multiple channels. Write your own plugin or install one from the registry, from email newsletters to IPFS and Nostr.',
    betaCta:'Request beta access',start:'Get started →',editor:'moss editor →',theme:'Writing a theme →',media:'Media file types →',requestType:'Request a new type',plugin:'Writing a plugin →',registry:'moss registry',
    closeH:'Your internet publisher is here.',closeB:"The internet is not only for browsing; it's also for creating. Now meet your internet publisher.",download:'Download moss',macNote:'macOS 12 or later',soon:'Coming soon',install:'Install from your terminal',copy:'Copy',copyNpm:'Copy npm install command',copyBrew:'Copy Homebrew install command',macTitle:'Download moss 0.14.1 for macOS (universal DMG)',windowsAria:'Windows desktop app, Coming soon',linuxAria:'Linux desktop app, Coming soon',
    betaH:'Help shape moss.',betaB:'Receive occasional updates and try moss’s default hosting service for free.',email:'Your email',request:'Request access',footer:'Footer',privacy:'Privacy',github:'moss on GitHub',language:'Language',
    editorFrame:'moss editor',previewFrame:'moss preview',notebook:'Jupyter notebook: a continuous Mandelbrot zoom',sketch:'an HTML sketch',article:'the published video article',pause:'Pause',play:'Play',pauseAria:'Pause the loop',playAria:'Play the loop',
    copied:'Copied to clipboard.',selected:'Command selected. Press Command-C or Ctrl-C to copy.',sending:'Sending…',success:'Thanks for applying. Confirm the email we just sent; we’ll email you when your turn comes.',offline:'Could not connect. Check your connection and try again.',error:'Something went wrong. Please try again.',
    docs:{start:'/get-started/',editor:'/get-started/editor/',theme:'/get-started/design/',media:'/get-started/links/',plugin:'/get-started/extend/',privacy:'https://mosspub.com/privacy'}
  };
  const hans = {
    ...en,product:'青苔',title:'青苔 — 从你的文件夹，到你的网站',description:'青苔将你的文件夹发布为网站。在本地文件夹里写 Markdown，用任意云盘协作。',
    intro:'从你的文件夹，<br>到你的网站',h1:'随处写作，<br>随处发布',b1:'青苔 将你的文件夹发布为网站。在本地文件夹里写 Markdown，用任意云盘协作。一键预览，再一键发布到任何地方。',h2:'强大如机器，<br>柔软如人',b2:'所有数据保存在本地文件中，你惯用的编辑器、代码助手皆可操作。直观的界面，让你的网站永远可读、易维护。',h3:'轻如文件夹，<br>广如互联网。',b3:'从文章、视频到 JavaScript 作品与 Jupyter 笔记本，青苔会优化文件并打包成网站。',h4:'用你的方式，<br>发布到你的互联网。',b4:'选择托管服务，将同一份内容同步到多个渠道。自行编写插件，或从插件目录安装，连接电子报、IPFS 与 Nostr。',
    betaCta:'申请内测',start:'开始使用 →',editor:'青苔编辑器 →',theme:'编写主题 →',media:'媒体文件类型 →',requestType:'申请文件支持',plugin:'编写插件 →',registry:'青苔插件目录',
    closeH:'你的互联网发布器。',closeB:'互联网不只用来浏览，也用来创造。这里是你的互联网发布器。',download:'下载青苔',macNote:'macOS 12 或更高版本',soon:'即将推出',install:'从终端安装',copy:'复制',copyNpm:'复制 npm 安装命令',copyBrew:'复制 Homebrew 安装命令',macTitle:'下载适用于 macOS 的青苔 0.14.1（通用 DMG）',windowsAria:'Windows 桌面应用，即将推出',linuxAria:'Linux 桌面应用，即将推出',
    betaH:'一起打磨青苔',betaB:'接收偶尔一次的更新消息，以及免费试用青苔默认托管服务',email:'你的邮箱',request:'申请内测',footer:'页脚',privacy:'隐私',github:'在 GitHub 查看青苔',language:'语言',
    editorFrame:'青苔编辑器',previewFrame:'青苔预览',notebook:'Jupyter 笔记本：连续放大的曼德博集合',sketch:'HTML 作品',article:'已发布的视频文章',pause:'暂停',play:'播放',pauseAria:'暂停循环视频',playAria:'播放循环视频',
    copied:'已复制到剪贴板。',selected:'已选中命令。按 Command-C 或 Ctrl-C 复制。',sending:'正在发送…',success:'感谢申请。请确认刚刚收到的邮件；轮到你时，我们会发信通知。',offline:'无法连接。请检查网络后重试。',error:'出现问题，请重试。',
    docs:{start:'/zh-hans/开始使用/',editor:'/zh-hans/开始使用/editor/',theme:'/zh-hans/开始使用/design/',media:'/zh-hans/开始使用/links/',plugin:'/zh-hans/开始使用/extend/',privacy:'https://mosspub.com/privacy'}
  };
  const hant = {
    ...en,product:'青苔',title:'青苔 — 從你的資料夾，到你的網站',description:'青苔將你的資料夾發布成網站。在本機資料夾裡寫 Markdown，用任何雲端硬碟協作。',
    intro:'從你的資料夾，<br>到你的網站。',h1:'隨處寫作，<br>隨處發布。',b1:'青苔將你的資料夾發布成網站。在本機資料夾裡寫 Markdown，用任何雲端硬碟協作。按一下就能預覽，再按一下就能發布到任何地方。',h2:'強大如機器，<br>柔軟如人。',b2:'所有資料都保存在本機檔案中，你慣用的編輯器、程式助理都能直接使用。直覺的介面，讓你的網站始終易讀、好維護。',h3:'輕如資料夾，<br>廣如網路。',b3:'從文章、影片到 JavaScript 作品與 Jupyter 筆記本，青苔會最佳化檔案並打包成網站。',h4:'用你的方式，<br>發布到你的網路。',b4:'選擇網站代管服務，將同一份內容同步到多個管道。自行編寫外掛，或從外掛目錄安裝，連接電子報、IPFS 與 Nostr。',
    betaCta:'申請封閉測試',start:'開始使用 →',editor:'青苔編輯器 →',theme:'編寫主題 →',media:'媒體檔案類型 →',requestType:'申請檔案支援',plugin:'編寫外掛 →',registry:'青苔外掛目錄',
    closeH:'你的網路發布器。',closeB:'網路不只用來瀏覽，也用來創作。這裡是你的網路發布器。',download:'下載青苔',macNote:'macOS 12 或更新版本',soon:'即將推出',install:'從終端機安裝',copy:'複製',copyNpm:'複製 npm 安裝指令',copyBrew:'複製 Homebrew 安裝指令',macTitle:'下載適用於 macOS 的青苔 0.14.1（通用 DMG）',windowsAria:'Windows 桌面應用程式，即將推出',linuxAria:'Linux 桌面應用程式，即將推出',
    betaH:'一起打磨青苔',betaB:'接收不定期的更新消息，並免費試用青苔的預設網站代管服務。',email:'你的電子郵件地址',request:'申請封閉測試',footer:'頁尾',privacy:'隱私權',github:'在 GitHub 查看青苔',language:'語言',
    editorFrame:'青苔編輯器',previewFrame:'青苔預覽',notebook:'Jupyter 筆記本：連續放大的曼德博集合',sketch:'HTML 作品',article:'已發布的影片文章',pause:'暫停',play:'播放',pauseAria:'暫停循環影片',playAria:'播放循環影片',
    copied:'已複製到剪貼簿。',selected:'已選取指令。按 Command-C 或 Ctrl-C 複製。',sending:'正在傳送…',success:'感謝你的申請。請先到信箱確認我們剛寄出的信件；開放體驗時，我們會再寄信通知你。',offline:'無法連線。請檢查網路連線後再試一次。',error:'發生問題，請再試一次。',
    docs:{start:'/zh-hant/開始使用/',editor:'/zh-hant/開始使用/editor/',theme:'/zh-hant/開始使用/design/',media:'/zh-hant/開始使用/links/',plugin:'/zh-hant/開始使用/extend/',privacy:'https://mosspub.com/privacy'}
  };
  const catalogs={en,'zh-hans':hans,'zh-hant':hant};
  const t=(key)=>catalogs[window.__LANDING_LOCALE||initial]?.[key]??en[key]??key;
  window.__landingI18n={t,locale:()=>window.__LANDING_LOCALE||initial};
  const text=(selector,key)=>{const n=document.querySelector(selector);if(n)n.textContent=t(key)};
  const html=(selector,key)=>{const n=document.querySelector(selector);if(n)n.innerHTML=t(key)};
  const attr=(selector,name,key)=>{const n=document.querySelector(selector);if(n)n.setAttribute(name,t(key))};
  function apply(next){
    window.__LANDING_LOCALE=next;window.__LANG=next==='en'?'en':'zh';const c=catalogs[next];
    document.documentElement.lang=next;document.title=c.title;document.querySelector('meta[name="description"]')?.setAttribute('content',c.description);text('.brand .moss-wordmark','product');
    html('#intro h1','intro');for(let i=1;i<=4;i++){html('#c'+i+' h2','h'+i);text('#c'+i+' .lede','b'+i)}
    const c1=document.querySelectorAll('#c1 .btns a');if(c1[0])c1[0].textContent=c.betaCta;if(c1[1]){c1[1].textContent=c.start;c1[1].href=c.docs.start}
    const c2=document.querySelectorAll('#c2 .btns a');if(c2[0]){c2[0].textContent=c.editor;c2[0].href=c.docs.editor}if(c2[1]){c2[1].textContent=c.theme;c2[1].href=c.docs.theme}
    const c3=document.querySelectorAll('#c3 .btns a');if(c3[0]){c3[0].textContent=c.media;c3[0].href=c.docs.media}if(c3[1])c3[1].textContent=c.requestType;
    const c4=document.querySelectorAll('#c4 .btns a');if(c4[0]){c4[0].textContent=c.plugin;c4[0].href=c.docs.plugin}if(c4[1])c4[1].textContent=c.registry;
    text('#five-headline','closeH');text('.closing-lede','closeB');attr('#downloads','aria-label','download');
    const notes=document.querySelectorAll('#downloads .availability');if(notes[0])notes[0].textContent=c.macNote;if(notes[1])notes[1].textContent=c.soon;if(notes[2])notes[2].textContent=c.soon;attr('[data-platform="macos"]','title','macTitle');attr('[data-platform="windows"]','aria-label','windowsAria');attr('[data-platform="linux"]','aria-label','linuxAria');
    attr('#commands','aria-label','install');document.querySelectorAll('.command button').forEach((b,i)=>{b.textContent=c.copy;b.setAttribute('aria-label',i?c.copyBrew:c.copyNpm)});
    text('#beta-title','betaH');text('#beta > div p','betaB');text('label[for="beta-email"]','email');attr('#beta-email','placeholder','email');text('#beta-form button','request');
    const footer=document.querySelector('#footer nav');footer?.setAttribute('aria-label',c.footer);const links=footer?.querySelectorAll('a');if(links?.[0]){links[0].textContent=c.privacy;links[0].href=c.docs.privacy}if(links?.[2])links[2].setAttribute('aria-label',c.github);
    attr('#ed','title','editorFrame');attr('#sh','title','previewFrame');attr('#vd','title','previewFrame');attr('#nb','title','notebook');attr('#sk','title','sketch');attr('#s3-site-frame','title','article');
    const toggle=document.querySelector('#loop-toggle');if(toggle){const playing=[en.pause,hans.pause,hant.pause].includes(toggle.textContent);toggle.textContent=playing?c.pause:c.play;toggle.setAttribute('aria-label',playing?c.pauseAria:c.playAria)}
    document.querySelector('.language-picker')?.setAttribute('aria-label',c.language);
    const status=document.querySelector('#beta-form .form-status');if(status?.dataset.messageKey)status.textContent=t(status.dataset.messageKey);
  }
  const key=url=>'moss-landing:'+url;
  const snap=()=>{const pending=!!document.querySelector('#beta-form button[aria-busy="true"]');return {y:scrollY,email:document.querySelector('#beta-email')?.value||'',status:pending?'':document.querySelector('#beta-form .form-status')?.textContent||'',state:pending?'':document.querySelector('#beta-form .form-status')?.dataset.state||'',messageKey:pending?'':document.querySelector('#beta-form .form-status')?.dataset.messageKey||''}};
  document.addEventListener('click',event=>{if(event.target.closest?.('[data-landing-locale]')&&document.querySelector('#beta-form button[aria-busy="true"]'))event.preventDefault()},true);
  const save=(url,state)=>{try{sessionStorage.setItem(key(url),JSON.stringify(state))}catch{}};
  const restore=()=>{try{const s=JSON.parse(sessionStorage.getItem(key(location.href))||'null');if(!s)return;const applyState=()=>{const input=document.querySelector('#beta-email');if(input)input.value=s.email;const status=document.querySelector('#beta-form .form-status');if(status){status.textContent=s.messageKey?t(s.messageKey):s.status;if(s.state)status.dataset.state=s.state;if(s.messageKey)status.dataset.messageKey=s.messageKey}scrollTo(0,s.y)};requestAnimationFrame(applyState);setTimeout(applyState,250)}catch{}};
  document.addEventListener('DOMContentLoaded',()=>{apply(initial);restore();document.querySelectorAll('a[href]').forEach(link=>{const url=new URL(link.href);if(url.origin!==location.origin){link.target='_blank';link.rel='noopener'}});document.querySelectorAll('[data-landing-locale]').forEach(link=>link.addEventListener('click',event=>{if(document.querySelector('#beta-form button[aria-busy="true"]')){event.preventDefault();return}const next=link.dataset.landingLocale;if(!supported.has(next)||next===window.__LANDING_LOCALE)return;const state=snap(),url=new URL(link.href);save(location.href,state);save(url.href,state);try{localStorage.setItem('moss-landing-locale',next)}catch{}}))});
})();
