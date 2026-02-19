function parsePgCookie(){
    var m=document.cookie.split(';').reduce(function(a,p){var kv=p.trim().split('=');a[kv[0]]=kv.slice(1).join('=');return a},{});
    var pg=m['pg']||'';
    return pg.split('&').reduce(function(a,p){var kv=p.split('=');a[kv[0]]=kv.slice(1).join('=');return a},{});
}
function saveState(){
    var ho=Array.from(document.querySelectorAll('.host-card[open]')).map(function(d){return d.dataset.addr}).filter(Boolean).join('|');
    var sc=Array.from(document.querySelectorAll('.svc-card[open]')).map(function(d){return d.dataset.title}).filter(Boolean).join('|');
    var open=document.querySelector('.svc-detail.open');
    var t=open?open.id:'';
    document.cookie='pg=ho='+ho+'&sc='+sc+'&t='+t+'; path=/; SameSite=Strict';
}
function openDetail(id,anchor){
    closeDetail();
    var d=document.getElementById(id);
    d.classList.add('open');
    document.getElementById('svc-backdrop').classList.add('open');
    if(window.innerWidth>768&&anchor){
        var r=anchor.getBoundingClientRect();
        var top=r.bottom+4;
        if(top+300>window.innerHeight){top=r.top-304}
        d.style.top=top+'px';
        d.style.left=Math.max(8,Math.min(r.left,window.innerWidth-320))+'px';
    }
    saveState();
}
function closeDetail(){
    document.querySelectorAll('.svc-detail.open').forEach(function(e){e.classList.remove('open');e.style.top='';e.style.left=''});
    document.getElementById('svc-backdrop').classList.remove('open');
    saveState();
}
document.querySelectorAll('.svc-item').forEach(function(el){
    el.addEventListener('click',function(){openDetail(el.dataset.svc,el)});
});
document.querySelectorAll('.svc-detail').forEach(function(d){
    d.addEventListener('click',function(e){e.stopPropagation()});
});
document.querySelectorAll('.svc-close').forEach(function(b){
    b.addEventListener('click',function(e){e.stopPropagation();closeDetail()});
});
document.getElementById('svc-backdrop').addEventListener('click',closeDetail);
document.querySelectorAll('.host-card,.svc-card').forEach(function(el){
    el.addEventListener('toggle',saveState);
});
(function(){
    var f=parsePgCookie();
    if(f['t']){
        var anchor=document.querySelector('[data-svc="'+f['t']+'"]');
        if(anchor)openDetail(f['t'],anchor);
    }
})();
