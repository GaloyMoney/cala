"use strict";(self.webpackChunkwebsite=self.webpackChunkwebsite||[]).push([[799],{3113:(e,c,n)=>{n.r(c),n.d(c,{assets:()=>s,contentTitle:()=>r,default:()=>u,frontMatter:()=>d,metadata:()=>t,toc:()=>l});var a=n(4848),o=n(8453);const d={id:"check-account-balance",title:"Check account balance",slug:"/docs/demo/check-account-balance"},r=void 0,t={id:"demo/check-account-balance",title:"Check account balance",description:"GraphQL body",source:"@site/docs/demo/check-account-balance.mdx",sourceDirName:"demo",slug:"/docs/demo/check-account-balance",permalink:"/docs/docs/demo/check-account-balance",draft:!1,unlisted:!1,editUrl:"https://github.com/GaloyMoney/cala/edit/main/docs/demo/check-account-balance.mdx",tags:[],version:"current",frontMatter:{id:"check-account-balance",title:"Check account balance",slug:"/docs/demo/check-account-balance"},sidebar:"demoSidebar",previous:{title:"Post a transaction",permalink:"/docs/docs/demo/post-transaction"}},s={},l=[{value:"GraphQL body",id:"graphql-body",level:3},{value:"Variables",id:"variables",level:3},{value:"Response",id:"response",level:3}];function i(e){const c={code:"code",h3:"h3",pre:"pre",...(0,o.R)(),...e.components};return(0,a.jsxs)(a.Fragment,{children:[(0,a.jsx)(c.h3,{id:"graphql-body",children:"GraphQL body"}),"\n",(0,a.jsx)(c.pre,{children:(0,a.jsx)(c.code,{className:"language-graphql",children:"query accountWithBalance($accountId: UUID!, $journalId: UUID!, $currency: CurrencyCode!) {\n  account(id: $accountId) {\n    name\n    balance(journalId: $journalId, currency: $currency) {\n      settled {\n        normalBalance {\n          units\n        }\n      }\n    }\n  }\n}\n"})}),"\n",(0,a.jsx)(c.h3,{id:"variables",children:"Variables"}),"\n",(0,a.jsx)(c.pre,{children:(0,a.jsx)(c.code,{children:'{\n  "accountId": "3a7d421b-7f5a-43ca-ba6f-5f3e6ee67237",\n  "journalId": "cfe09e8f-3228-4444-a50d-884d11a74cd0",\n  "currency": "USD"\n}\n'})}),"\n",(0,a.jsx)(c.h3,{id:"response",children:"Response"}),"\n",(0,a.jsx)(c.pre,{children:(0,a.jsx)(c.code,{className:"language-json",children:"{}\n"})})]})}function u(e={}){const{wrapper:c}={...(0,o.R)(),...e.components};return c?(0,a.jsx)(c,{...e,children:(0,a.jsx)(i,{...e})}):i(e)}}}]);