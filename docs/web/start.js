/* Start page — email step of the trial funnel.

   Front-end only for now. When the backend lands, replace the body of
   `submit()` with a POST to the auth service; on success the user gets a
   magic link, and the link's callback opens Stripe Checkout. */

const form = document.getElementById('gate-form');
const input = document.getElementById('email');
const errorEl = document.getElementById('gate-error');
const doneEl = document.getElementById('gate-done');

const VALID = /^[^\s@]+@[^\s@]+\.[^\s@]{2,}$/;

if (form) {
  form.addEventListener('submit', (ev) => {
    ev.preventDefault();

    const email = input.value.trim();

    if (!VALID.test(email)) {
      errorEl.hidden = false;
      input.setAttribute('aria-invalid', 'true');
      input.focus();
      return;
    }

    errorEl.hidden = true;
    input.removeAttribute('aria-invalid');

    // → POST /api/auth/magic-link { email } once the service exists.
    form.hidden = true;
    doneEl.hidden = false;
    doneEl.innerHTML =
      'Check <b>' + email.replace(/[&<>"]/g, '') + '</b>. ' +
      'The link signs you in and takes you straight to checkout.';
    doneEl.focus();
  });

  input.addEventListener('input', () => {
    if (!errorEl.hidden) {
      errorEl.hidden = true;
      input.removeAttribute('aria-invalid');
    }
  });
}
