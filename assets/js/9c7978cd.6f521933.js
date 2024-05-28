"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[447],{8327:(e,t,n)=>{n.r(t),n.d(t,{assets:()=>i,contentTitle:()=>c,default:()=>p,frontMatter:()=>s,metadata:()=>r,toc:()=>d});var a=n(4848),o=n(8453);const s={id:"post-transaction",title:"Post a transaction",slug:"/docs/demo/post-transaction"},c=void 0,r={id:"demo/post-transaction",title:"Post a transaction",description:"GraphQL body",source:"@site/docs/demo/post-transaction.mdx",sourceDirName:"demo",slug:"/docs/demo/post-transaction",permalink:"/docs/docs/demo/post-transaction",draft:!1,unlisted:!1,editUrl:"https://github.com/GaloyMoney/cala/edit/main/docs/demo/post-transaction.mdx",tags:[],version:"current",frontMatter:{id:"post-transaction",title:"Post a transaction",slug:"/docs/demo/post-transaction"},sidebar:"demoSidebar",previous:{title:"Create journal and accounts",permalink:"/docs/docs/demo/create-journal-and-accounts"},next:{title:"Create a transaction template",permalink:"/docs/docs/demo/tx-template-create"}},i={},d=[{value:"GraphQL body",id:"graphql-body",level:3},{value:"Variables",id:"variables",level:3},{value:"Response",id:"response",level:3}];function l(e){const t={code:"code",h3:"h3",pre:"pre",...(0,o.R)(),...e.components};return(0,a.jsxs)(a.Fragment,{children:[(0,a.jsx)(t.h3,{id:"graphql-body",children:"GraphQL body"}),"\n",(0,a.jsx)(t.pre,{children:(0,a.jsx)(t.code,{className:"language-graphql",children:"mutation accountCreate($input: AccountCreateInput!) {\n  accountCreate(input: $input) {\n    account {\n      accountId\n      name\n    }\n  }\n}\n"})}),"\n",(0,a.jsx)(t.h3,{id:"variables",children:"Variables"}),"\n",(0,a.jsx)(t.pre,{children:(0,a.jsx)(t.code,{children:'input: {\n  transactionId: transaction_id,\n  txTemplateCode: `DEPOSIT-${depositTemplateId}`,\n  params: {\n    account: account_id,\n    amount: "9.53",\n    effective: "2022-09-21"\n  }\n}\n'})}),"\n",(0,a.jsx)(t.h3,{id:"response",children:"Response"}),"\n",(0,a.jsx)(t.pre,{children:(0,a.jsx)(t.code,{className:"language-json",children:"{}\n"})})]})}function p(e={}){const{wrapper:t}={...(0,o.R)(),...e.components};return t?(0,a.jsx)(t,{...e,children:(0,a.jsx)(l,{...e})}):l(e)}}}]);