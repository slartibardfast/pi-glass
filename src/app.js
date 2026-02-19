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
}
function closeDetail(){
    document.querySelectorAll('.svc-detail.open').forEach(function(e){e.classList.remove('open');e.style.top='';e.style.left=''});
    document.getElementById('svc-backdrop').classList.remove('open');
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
