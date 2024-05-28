"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[238],{3215:(e,n,a)=>{a.r(n),a.d(n,{assets:()=>s,contentTitle:()=>t,default:()=>u,frontMatter:()=>l,metadata:()=>o,toc:()=>d});var c=a(4848),r=a(8453);const l={id:"create-journal-and-accounts",title:"Create journal and accounts",slug:"/docs/demo/create-journal-and-accounts"},t=void 0,o={id:"demo/create-journal-and-accounts",title:"Create journal and accounts",description:"Create a journal",source:"@site/docs/demo/create-journal-and-accounts.mdx",sourceDirName:"demo",slug:"/docs/demo/create-journal-and-accounts",permalink:"/docs/docs/demo/create-journal-and-accounts",draft:!1,unlisted:!1,editUrl:"https://github.com/GaloyMoney/cala/edit/main/docs/demo/create-journal-and-accounts.mdx",tags:[],version:"current",frontMatter:{id:"create-journal-and-accounts",title:"Create journal and accounts",slug:"/docs/demo/create-journal-and-accounts"},sidebar:"demoSidebar",previous:{title:"Try Cala",permalink:"/docs/"},next:{title:"Post a transaction",permalink:"/docs/docs/demo/post-transaction"}},s={},d=[{value:"Create a journal",id:"create-a-journal",level:2},{value:"GraphQL body",id:"graphql-body",level:3},{value:"Variables",id:"variables",level:3},{value:"Response",id:"response",level:3},{value:"Create a checking account",id:"create-a-checking-account",level:2},{value:"GraphQL body",id:"graphql-body-1",level:3},{value:"Variables",id:"variables-1",level:3},{value:"Response",id:"response-1",level:3},{value:"Create a debit account",id:"create-a-debit-account",level:2},{value:"GraphQL body",id:"graphql-body-2",level:3},{value:"Variables",id:"variables-2",level:3},{value:"Response",id:"response-2",level:3}];function i(e){const n={code:"code",h2:"h2",h3:"h3",pre:"pre",...(0,r.R)(),...e.components};return(0,c.jsxs)(c.Fragment,{children:[(0,c.jsx)(n.h2,{id:"create-a-journal",children:"Create a journal"}),"\n",(0,c.jsx)(n.h3,{id:"graphql-body",children:"GraphQL body"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-graphql",children:"mutation journalCreate($input: JournalCreateInput!) {\n  journalCreate(input: $input) {\n    journal {\n      journalId\n      name\n    }\n  }\n}\n"})}),"\n",(0,c.jsx)(n.h3,{id:"variables",children:"Variables"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{children:"input: {\n  journalId: journal_id,\n  name: 'Your Journal Name'\n}\n"})}),"\n",(0,c.jsx)(n.h3,{id:"response",children:"Response"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-json",children:"{}\n"})}),"\n",(0,c.jsx)(n.h2,{id:"create-a-checking-account",children:"Create a checking account"}),"\n",(0,c.jsx)(n.h3,{id:"graphql-body-1",children:"GraphQL body"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-graphql",children:"mutation accountCreate($input: AccountCreateInput!) {\n  accountCreate(input: $input) {\n    account {\n      accountId\n      name\n    }\n  }\n}\n"})}),"\n",(0,c.jsx)(n.h3,{id:"variables-1",children:"Variables"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-json",children:'input: {\n  accountId: liability_account_id,\n  name: "Alice - Checking",\n  code: `ALICE.CHECKING-${liability_account_id}`,\n  normalBalanceType: "CREDIT",\n}\n'})}),"\n",(0,c.jsx)(n.h3,{id:"response-1",children:"Response"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-json",children:"{}\n"})}),"\n",(0,c.jsx)(n.h2,{id:"create-a-debit-account",children:"Create a debit account"}),"\n",(0,c.jsx)(n.h3,{id:"graphql-body-2",children:"GraphQL body"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-graphql",children:"mutation accountCreate($input: AccountCreateInput!) {\n  accountCreate(input: $input) {\n    account {\n      accountId\n      name\n    }\n  }\n}\n"})}),"\n",(0,c.jsx)(n.h3,{id:"variables-2",children:"Variables"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{children:'input: {\n  accountId: asset_account_id,\n  name: "Assets",\n  code: `ASSET-${asset_account_id}`,\n  normalBalanceType: "DEBIT",\n}\n'})}),"\n",(0,c.jsx)(n.h3,{id:"response-2",children:"Response"}),"\n",(0,c.jsx)(n.pre,{children:(0,c.jsx)(n.code,{className:"language-json",children:"{}\n"})})]})}function u(e={}){const{wrapper:n}={...(0,r.R)(),...e.components};return n?(0,c.jsx)(n,{...e,children:(0,c.jsx)(i,{...e})}):i(e)}}}]);