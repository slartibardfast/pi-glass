function saveState(){
    var ho=Array.from(document.querySelectorAll('.host-card[open]')).map(function(d){return d.dataset.addr}).filter(Boolean).join('|');
    var sc=Array.from(document.querySelectorAll('.svc-card[open]')).map(function(d){return d.dataset.title}).filter(Boolean).join('|');
    var si=Array.from(document.querySelectorAll('.svc-item[open]')).map(function(d){return d.dataset.svc}).filter(Boolean).join('|');
    var th=document.documentElement.dataset.theme||'';
    document.cookie='pg=ho='+ho+'&sc='+sc+'&si='+si+'&th='+th+'; path=/; SameSite=Strict';
}
document.querySelectorAll('.host-card,.svc-card,.svc-item').forEach(function(el){
    el.addEventListener('toggle',saveState);
});
(function(){
    var btn=document.getElementById('theme-btn');
    if(!btn)return;
    var root=document.documentElement;
    var order=['auto','dark','light'];
    var icons={'auto':'⊙','dark':'☾','light':'☀'};
    var tips={'auto':'Theme: auto (follows system)','dark':'Theme: dark','light':'Theme: light'};
    function cur(){return root.dataset.theme||'auto';}
    var t=cur();btn.textContent=icons[t];btn.title=tips[t];
    btn.addEventListener('click',function(){
        var next=order[(order.indexOf(cur())+1)%order.length];
        if(next==='auto'){delete root.dataset.theme;}else{root.dataset.theme=next;}
        btn.textContent=icons[next];btn.title=tips[next];
        saveState();
    });
}());
