// Paper mode (#236): the Print button on the worksheet page opens the
// browser's print dialog. The page has no other script of its own.
document.querySelectorAll('[data-print]').forEach(function (b) {
  b.addEventListener('click', function () { window.print(); });
});
