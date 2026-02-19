function saveState(){
    var ho=Array.from(document.querySelectorAll('.host-card[open]')).map(function(d){return d.dataset.addr}).filter(Boolean).join('|');
    var sc=Array.from(document.querySelectorAll('.svc-card[open]')).map(function(d){return d.dataset.title}).filter(Boolean).join('|');
    var si=Array.from(document.querySelectorAll('.svc-item[open]')).map(function(d){return d.dataset.svc}).filter(Boolean).join('|');
    document.cookie='pg=ho='+ho+'&sc='+sc+'&si='+si+'; path=/; SameSite=Strict';
}
document.querySelectorAll('.host-card,.svc-card,.svc-item').forEach(function(el){
    el.addEventListener('toggle',saveState);
});
