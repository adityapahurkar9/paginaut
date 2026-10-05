// The Paginaut mark (assets/icon.svg): a P that is also a page, an astronaut's helmet in its bowl.
// Used by both apps (the Android viewer and the Windows title bar and start page).

/** `tile`: on a white rounded square, as the app icon is, so it reads on dark title bars too. */
export function Logo({ size = 32, tile = false }: { size?: number; tile?: boolean }) {
  return (
    <svg width={size} height={size} viewBox={tile ? "-95 -38 1100 1100" : "160 100 704 824"} aria-hidden="true" class="logo">
      {tile && <rect x="-95" y="-38" width="1100" height="1100" rx="240" fill="#fff" />}
      <defs>
        <linearGradient id="lg-navy" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#1a2c63" />
          <stop offset="1" stop-color="#0d1a40" />
        </linearGradient>
        <linearGradient id="lg-fold" x1="1" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#2f7bff" />
          <stop offset="1" stop-color="#0a4ad8" />
        </linearGradient>
        <clipPath id="lg-above">
          <path d="M0 0H1024V600L0 432Z" />
        </clipPath>
      </defs>
      <path fill="url(#lg-navy)" d="M210 112H578A278 278 0 0 1 623 664L392 912H210Q181 912 181 883V141Q181 112 210 112Z" />
      <g clip-path="url(#lg-above)">
        <circle cx="474" cy="390" r="177" fill="#fff" />
      </g>
      <circle cx="521" cy="372" r="110" fill="#0a1433" />
      <path d="M498 286A88 88 0 0 1 600 344" fill="none" stroke="#fff" stroke-width="18" stroke-linecap="round" />
      <circle cx="370" cy="412" r="52" fill="#fff" />
      <circle cx="370" cy="412" r="36" fill="none" stroke="#0d1a40" stroke-width="10" />
      <path fill="#fff" d="M684 185Q690 225 730 231Q690 237 684 277Q678 237 638 231Q678 225 684 185Z" />
      <path fill="#fff" d="M282 497L607 555Q623 558 623 574V648Q566 650 515 664L328 865H282Q252 865 252 835V527Q252 497 282 497Z" />
      <path fill="url(#lg-fold)" d="M515 664Q566 650 623 648V664L436 865H328Z" />
      <path d="M306 576H480M306 628H466M306 680H440" fill="none" stroke="#8a98c0" stroke-width="24" stroke-linecap="round" />
    </svg>
  );
}
